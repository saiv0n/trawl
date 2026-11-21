//! HLS/DASH manifest okuma; popup ve video panelindeki "1080p · 2 sa · ~1,8 GB" satırı buradan.

use crate::util::client;
use anyhow::{bail, Result};
use regex::Regex;
use serde::Serialize;
use std::collections::BTreeMap;
use std::time::Duration;
use url::Url;

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Variant {
    pub url: String,
    pub bandwidth: u64,
    pub width: u32,
    pub height: u32,
    pub codecs: String,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct StreamInfo {
    pub kind: String, // "hls" | "dash"
    pub master: bool,
    pub variants: Vec<Variant>, // büyükten küçüğe (bant genişliği)
    pub audio: Vec<String>,     // ayrı ses playlist'leri (HLS)
    pub duration: f64,          // saniye, 0 canlı/bilinmiyor
    pub live: bool,
    pub encrypted: bool, // AES-128 vb., indirilebilir
    pub drm: bool,       // widevine/playready/fairplay/sample-aes, indirilemez
    pub height: u32,     // en iyi varyant
    pub size: u64,       // tahmini bayt, 0 bilinmiyor
}

/// HLS öznitelik listesi, KEY=VAL,KEY="v,al"
fn attrs(s: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let ks = i;
        while i < b.len() && b[i] != b'=' {
            i += 1;
        }
        let k = s[ks..i].trim().trim_start_matches(',').trim().to_ascii_uppercase();
        i += 1;
        let v;
        if i < b.len() && b[i] == b'"' {
            let vs = i + 1;
            i = vs;
            while i < b.len() && b[i] != b'"' {
                i += 1;
            }
            v = s[vs..i.min(b.len())].to_string();
            i += 1;
        } else {
            let vs = i.min(b.len());
            while i < b.len() && b[i] != b',' {
                i += 1;
            }
            v = s[vs..i].trim().to_string();
        }
        while i < b.len() && b[i] == b',' {
            i += 1;
        }
        if !k.is_empty() {
            out.insert(k, v);
        }
    }
    out
}

fn join(base: &Url, u: &str) -> String {
    base.join(u.trim()).map(|x| x.to_string()).unwrap_or_else(|_| u.trim().to_string())
}

fn key_is_drm(a: &BTreeMap<String, String>) -> (bool, bool) {
    let method = a.get("METHOD").map(|s| s.to_ascii_uppercase()).unwrap_or_default();
    if method.is_empty() || method == "NONE" {
        return (false, false);
    }
    let fmt = a.get("KEYFORMAT").map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    let uri = a.get("URI").map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    let drm = method.starts_with("SAMPLE-AES")
        || fmt.contains("widevine")
        || fmt.contains("playready")
        || fmt.contains("streamingkeydelivery")
        || fmt.starts_with("urn:uuid:")
        || uri.starts_with("skd://")
        || uri.starts_with("data:text/plain;base64,"); // PSSH gömülü
    (true, drm)
}

pub fn parse_m3u8(base: &str, text: &str) -> StreamInfo {
    let base = Url::parse(base).unwrap_or_else(|_| Url::parse("http://x/").unwrap());
    let mut si = StreamInfo { kind: "hls".into(), live: true, ..Default::default() };
    let mut pending: Option<BTreeMap<String, String>> = None;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(rest) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            pending = Some(attrs(rest));
            si.master = true;
        } else if let Some(rest) = line.strip_prefix("#EXT-X-MEDIA:") {
            let a = attrs(rest);
            if a.get("TYPE").map(|t| t == "AUDIO").unwrap_or(false) {
                if let Some(u) = a.get("URI") {
                    let u = join(&base, u);
                    if !si.audio.contains(&u) {
                        si.audio.push(u);
                    }
                }
            }
        } else if let Some(rest) = line.strip_prefix("#EXT-X-KEY:").or_else(|| line.strip_prefix("#EXT-X-SESSION-KEY:")) {
            let (enc, drm) = key_is_drm(&attrs(rest));
            si.encrypted |= enc;
            si.drm |= drm;
        } else if let Some(rest) = line.strip_prefix("#EXTINF:") {
            si.duration += rest.split(',').next().and_then(|d| d.trim().parse::<f64>().ok()).unwrap_or(0.0);
        } else if line == "#EXT-X-ENDLIST" || line.eq_ignore_ascii_case("#EXT-X-PLAYLIST-TYPE:VOD") {
            si.live = false;
        } else if !line.starts_with('#') {
            if let Some(a) = pending.take() {
                let (w, h) = a
                    .get("RESOLUTION")
                    .and_then(|r| r.split_once(['x', 'X']))
                    .map(|(w, h)| (w.parse().unwrap_or(0), h.parse().unwrap_or(0)))
                    .unwrap_or((0, 0));
                let bw = a.get("BANDWIDTH").or_else(|| a.get("AVERAGE-BANDWIDTH")).and_then(|b| b.parse().ok()).unwrap_or(0);
                let v = Variant { url: join(&base, line), bandwidth: bw, width: w, height: h, codecs: a.get("CODECS").cloned().unwrap_or_default() };
                if !si.variants.iter().any(|x| x.url == v.url) {
                    si.variants.push(v);
                }
            }
        }
    }
    if si.master {
        si.live = false; // master'da ENDLIST olmaz, varyanta bakılır
    }
    si.variants.sort_by(|a, b| b.bandwidth.cmp(&a.bandwidth).then(b.height.cmp(&a.height)));
    si.height = si.variants.first().map(|v| v.height).unwrap_or(0);
    si
}

/// ISO 8601 süreyi (PT1H2M3.5S) saniyeye çevirir
fn iso_dur(s: &str) -> f64 {
    let re = Regex::new(r"(?i)P(?:(\d+)D)?T?(?:(\d+)H)?(?:(\d+)M)?(?:([\d.]+)S)?").unwrap();
    let Some(c) = re.captures(s) else { return 0.0 };
    let g = |i: usize| c.get(i).and_then(|m| m.as_str().parse::<f64>().ok()).unwrap_or(0.0);
    g(1) * 86400.0 + g(2) * 3600.0 + g(3) * 60.0 + g(4)
}

pub fn parse_mpd(base: &str, text: &str) -> StreamInfo {
    let mut si = StreamInfo { kind: "dash".into(), master: true, ..Default::default() };
    let attr = |tag: &str, k: &str| -> Option<String> {
        Regex::new(&format!(r#"(?i)\b{k}\s*=\s*"([^"]*)""#)).unwrap().captures(tag).map(|c| c[1].to_string())
    };
    let mpd_tag = Regex::new(r"(?is)<MPD\b[^>]*>").unwrap().find(text).map(|m| m.as_str().to_string()).unwrap_or_default();
    si.duration = attr(&mpd_tag, "mediaPresentationDuration").map(|d| iso_dur(&d)).unwrap_or(0.0);
    si.live = attr(&mpd_tag, "type").map(|t| t.eq_ignore_ascii_case("dynamic")).unwrap_or(false);
    // cenc tek başına da DRM, anahtar lisans sunucusundan geliyor
    si.drm = Regex::new(r"(?i)<ContentProtection\b").unwrap().is_match(text);
    si.encrypted = si.drm;
    // mimeType/codecs AdaptationSet'ten Representation'a miras
    let set_re = Regex::new(r"(?is)<AdaptationSet\b([^>]*)>(.*?)</AdaptationSet>").unwrap();
    let rep_re = Regex::new(r"(?is)<Representation\b[^>]*>").unwrap();
    for set in set_re.captures_iter(text) {
        let sa = &set[1];
        let smime = attr(sa, "mimeType").or_else(|| attr(sa, "contentType")).unwrap_or_default();
        for rep in rep_re.find_iter(&set[2]) {
            let r = rep.as_str();
            let mime = attr(r, "mimeType").unwrap_or_else(|| smime.clone());
            let v = Variant {
                url: base.to_string(),
                bandwidth: attr(r, "bandwidth").and_then(|b| b.parse().ok()).unwrap_or(0),
                width: attr(r, "width").and_then(|b| b.parse().ok()).unwrap_or(0),
                height: attr(r, "height").and_then(|b| b.parse().ok()).unwrap_or(0),
                codecs: attr(r, "codecs").or_else(|| attr(sa, "codecs")).unwrap_or_default(),
            };
            if mime.contains("audio") {
                // ses bant genişliğini boyut tahmini için ayrı tut
                si.audio.push(v.bandwidth.to_string());
            } else if v.height > 0 || mime.contains("video") {
                si.variants.push(v);
            }
        }
    }
    si.variants.sort_by(|a, b| b.bandwidth.cmp(&a.bandwidth).then(b.height.cmp(&a.height)));
    si.height = si.variants.first().map(|v| v.height).unwrap_or(0);
    let abw: u64 = si.audio.iter().filter_map(|a| a.parse::<u64>().ok()).max().unwrap_or(0);
    si.audio.clear();
    if let Some(v) = si.variants.first() {
        si.size = ((v.bandwidth + abw) as f64 * si.duration / 8.0) as u64;
    }
    si
}

async fn fetch_text(url: &str, h: &BTreeMap<String, String>) -> Result<(String, String)> {
    let r = crate::http::hdrs(client().get(url), h).timeout(Duration::from_secs(20)).send().await?;
    if r.status().as_u16() >= 400 {
        bail!("Sunucu {} döndü", r.status().as_u16());
    }
    let fin = r.url().to_string();
    let b = r.bytes().await?;
    if b.len() > 8 << 20 {
        bail!("Manifest çok büyük");
    }
    Ok((fin, String::from_utf8_lossy(&b).to_string()))
}

pub async fn probe(url: &str, h: &BTreeMap<String, String>) -> Result<StreamInfo> {
    let (fin, text) = fetch_text(url, h).await?;
    let t = text.trim_start_matches('\u{feff}').trim_start();
    if t.starts_with("#EXTM3U") {
        let mut si = parse_m3u8(&fin, t);
        if si.master {
            // süre ve DRM için en iyi varyantın playlist'ine bak
            if let Some(best) = si.variants.first().cloned() {
                if let Ok((vf, vt)) = fetch_text(&best.url, h).await {
                    let v = parse_m3u8(&vf, &vt);
                    si.duration = v.duration;
                    si.live = v.live;
                    si.encrypted |= v.encrypted;
                    si.drm |= v.drm;
                }
                if si.duration > 0.0 {
                    // ses ayrıysa da BANDWIDTH toplamı veriyor (HLS böyle tanımlıyor)
                    si.size = (best.bandwidth as f64 * si.duration / 8.0) as u64;
                }
            }
        }
        return Ok(si);
    }
    if t.contains("<MPD") {
        return Ok(parse_mpd(&fin, t));
    }
    bail!("HLS ya da DASH manifesti değil")
}

#[cfg(test)]
mod tests {
    use super::*;

    const MASTER: &str = "#EXTM3U
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"Türkçe\",URI=\"audio/tr.m3u8\"
#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360,CODECS=\"avc1.4d401e,mp4a.40.2\",AUDIO=\"aud\"
360/index.m3u8
#EXT-X-I-FRAME-STREAM-INF:BANDWIDTH=90000,URI=\"iframe.m3u8\"
#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080,CODECS=\"avc1.640028,mp4a.40.2\"
https://cdn.example.com/1080/index.m3u8?tok=a,b
";

    #[test]
    fn master_variants_sorted_and_resolved() {
        let si = parse_m3u8("https://site.tv/v/master.m3u8", MASTER);
        assert!(si.master);
        assert_eq!(si.variants.len(), 2);
        assert_eq!(si.variants[0].height, 1080);
        assert_eq!(si.variants[0].url, "https://cdn.example.com/1080/index.m3u8?tok=a,b");
        assert_eq!(si.variants[1].url, "https://site.tv/v/360/index.m3u8");
        assert_eq!(si.variants[1].codecs, "avc1.4d401e,mp4a.40.2");
        assert_eq!(si.audio, vec!["https://site.tv/v/audio/tr.m3u8"]);
        assert!(!si.drm);
    }

    #[test]
    fn media_playlist_duration_and_keys() {
        let aes = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"k.bin\"\n#EXTINF:6.0,\na.ts\n#EXTINF:4.5,\nb.ts\n#EXT-X-ENDLIST\n";
        let si = parse_m3u8("https://x/y.m3u8", aes);
        assert!(!si.master && !si.live && si.encrypted && !si.drm);
        assert!((si.duration - 10.5).abs() < 1e-9);
        let wv = "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://abc\",KEYFORMAT=\"com.apple.streamingkeydelivery\"\n#EXTINF:6,\na.ts\n";
        let si = parse_m3u8("https://x/y.m3u8", wv);
        assert!(si.drm && si.live);
    }

    #[test]
    fn mpd_duration_size_drm() {
        let mpd = r#"<?xml version="1.0"?><MPD type="static" mediaPresentationDuration="PT1H0M0.0S">
<Period><AdaptationSet mimeType="video/mp4"><Representation id="1" bandwidth="4000000" width="1920" height="1080" codecs="avc1"/>
<Representation id="2" bandwidth="1000000" width="854" height="480"/></AdaptationSet>
<AdaptationSet mimeType="audio/mp4"><Representation id="a" bandwidth="128000"/></AdaptationSet></Period></MPD>"#;
        let si = parse_mpd("https://x/m.mpd", mpd);
        assert_eq!(si.duration, 3600.0);
        assert_eq!(si.height, 1080);
        assert_eq!(si.size, (4_128_000.0 * 3600.0 / 8.0) as u64);
        assert!(!si.drm && !si.live);
        let p = mpd.replace("<Period>", "<Period><ContentProtection schemeIdUri=\"urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed\"/>");
        assert!(parse_mpd("https://x/m.mpd", &p).drm);
    }

    #[tokio::test]
    #[ignore = "ağ gerektirir"]
    async fn probe_apple_bipbop() {
        let u = "https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_fmp4/master.m3u8";
        let si = probe(u, &BTreeMap::new()).await.unwrap();
        assert!(si.master && !si.drm && si.height >= 1080 && si.duration > 60.0 && si.size > 0, "{si:?}");
    }

    #[test]
    fn attr_parser_handles_quotes() {
        let a = attrs("BANDWIDTH=1,CODECS=\"a,b\",RESOLUTION=1x2");
        assert_eq!(a["CODECS"], "a,b");
        assert_eq!(a["RESOLUTION"], "1x2");
    }
}
