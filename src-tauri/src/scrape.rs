//! sayfadaki (istenirse aynı sitedeki alt sayfalardaki) indirilebilir her şeyi bulur

use crate::util::*;
use anyhow::Result;
use futures_util::{stream, StreamExt};
use regex::Regex;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::OnceLock;
use std::time::Duration;
use url::Url;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Item {
    pub url: String,
    pub kind: String,
    pub name: String,
    pub ext: String,
    pub size: u64,
    pub mime: String,
    pub source: String,
    pub thumb: String,
    pub title: String,
    pub page: String,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct ScrapeResult {
    pub url: String,
    pub title: String,
    pub icon: String,
    pub pages: usize,
    pub items: Vec<Item>,
}

const STREAM: &[&str] = &["m3u8", "mpd"];

pub fn kind_of_ext(e: &str) -> &'static str {
    if STREAM.contains(&e) {
        return "stream";
    }
    match category_of(&format!("x.{e}")).as_str() {
        "video" => "video",
        "muzik" => "audio",
        "resim" => "image",
        "belge" => "doc",
        "arsiv" => "archive",
        "program" => "app",
        _ => "other",
    }
}

fn kind_of_mime(m: &str) -> &'static str {
    if m.contains("mpegurl") || m.contains("dash+xml") {
        "stream"
    } else if m.starts_with("video/") {
        "video"
    } else if m.starts_with("audio/") {
        "audio"
    } else if m.starts_with("image/") {
        "image"
    } else if m == "application/pdf" || m.contains("officedocument") || m.contains("msword") {
        "doc"
    } else if m.contains("zip") || m.contains("rar") || m.contains("7z") || m.contains("tar") {
        "archive"
    } else if m.contains("msdownload") || m.contains("android.package") || m.contains("x-msi") {
        "app"
    } else {
        ""
    }
}

fn re_url() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        let exts = "mp4|m4v|webm|mkv|mov|avi|flv|m3u8|mpd|mp3|m4a|aac|flac|wav|ogg|opus|pdf|zip|rar|7z|exe|msi|apk|iso|dmg|epub|docx?|xlsx?|pptx?|gif|webp|jpe?g|png|avif|ts";
        Regex::new(&format!(r#"(?i)(https?://[^\s"'<>()\\]+?\.(?:{exts})\b(?:\?[^\s"'<>()\\]*)?)"#)).unwrap()
    })
}

fn sel(s: &str) -> Selector {
    Selector::parse(s).unwrap()
}

fn absolutize(base: &Url, raw: &str) -> Option<String> {
    let raw = raw.trim().replace("\\/", "/").replace("\\u002F", "/").replace("&amp;", "&");
    if raw.is_empty() || raw.starts_with("data:") || raw.starts_with("javascript:") || raw.starts_with("blob:")
        || raw.starts_with('#') || raw.starts_with("mailto:")
    {
        return None;
    }
    let u = base.join(&raw).ok()?;
    if u.scheme() != "http" && u.scheme() != "https" {
        return None;
    }
    let mut u = u;
    u.set_fragment(None);
    Some(u.to_string())
}

fn best_srcset(s: &str) -> Option<String> {
    s.split(',')
        .filter_map(|p| {
            let mut it = p.trim().split_whitespace();
            let u = it.next()?.to_string();
            let w = it.next().unwrap_or("1x");
            let n: f64 = w.trim_end_matches(|c| c == 'w' || c == 'x').parse().unwrap_or(1.0);
            Some((u, n))
        })
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|x| x.0)
}

struct Collector {
    base: Url,
    page: String,
    items: Vec<Item>,
    seen: HashSet<String>,
}

impl Collector {
    fn push(&mut self, raw: &str, source: &str, kind_hint: &str, title: &str, thumb: &str) {
        let Some(u) = absolutize(&self.base, raw) else { return };
        if !self.seen.insert(u.clone()) {
            if !title.is_empty() || !thumb.is_empty() {
                if let Some(it) = self.items.iter_mut().find(|i| i.url == u) {
                    if it.title.is_empty() {
                        it.title = title.into();
                    }
                    if it.thumb.is_empty() {
                        it.thumb = thumb.into();
                    }
                }
            }
            return;
        }
        let ext = url_ext(&u);
        let mut kind = kind_of_ext(&ext).to_string();
        if kind == "other" && !kind_hint.is_empty() {
            kind = kind_hint.into();
        }
        if kind == "other" {
            return;
        }
        let name = safe_name(&name_from_url(&u));
        self.items.push(Item {
            url: u,
            kind,
            name,
            ext,
            source: source.into(),
            title: title.trim().chars().take(160).collect(),
            thumb: thumb.into(),
            page: self.page.clone(),
            ..Default::default()
        });
    }
}

pub struct Page {
    pub title: String,
    pub icon: String,
    pub items: Vec<Item>,
    pub links: Vec<String>,
}

pub fn parse(page_url: &str, html: &str) -> Page {
    let base = Url::parse(page_url).unwrap_or_else(|_| Url::parse("https://localhost/").unwrap());
    let doc = Html::parse_document(html);
    let mut c = Collector { base: base.clone(), page: page_url.into(), items: vec![], seen: HashSet::new() };
    let base = doc
        .select(&sel("base[href]"))
        .next()
        .and_then(|b| b.value().attr("href"))
        .and_then(|h| base.join(h).ok())
        .unwrap_or(base);
    c.base = base.clone();

    let title = doc.select(&sel("title")).next().map(|t| t.text().collect::<String>()).unwrap_or_default();
    let mut icon = String::new();
    let mut og_image = String::new();

    for m in doc.select(&sel("meta[content]")) {
        let v = m.value();
        let key = v.attr("property").or(v.attr("name")).or(v.attr("itemprop")).unwrap_or("").to_lowercase();
        let ct = v.attr("content").unwrap_or("");
        match key.as_str() {
            "og:video" | "og:video:url" | "og:video:secure_url" | "twitter:player:stream" | "contenturl" => {
                c.push(ct, "meta", "video", &title, "")
            }
            "og:audio" | "og:audio:url" | "og:audio:secure_url" => c.push(ct, "meta", "audio", &title, ""),
            "og:image" | "og:image:url" | "og:image:secure_url" | "twitter:image" | "thumbnailurl" => {
                if og_image.is_empty() {
                    og_image = absolutize(&base, ct).unwrap_or_default();
                }
                c.push(ct, "meta", "image", &title, "")
            }
            _ => {}
        }
    }
    for l in doc.select(&sel("link[href]")) {
        let v = l.value();
        let rel = v.attr("rel").unwrap_or("").to_lowercase();
        let h = v.attr("href").unwrap_or("");
        if rel.contains("icon") && icon.is_empty() {
            icon = absolutize(&base, h).unwrap_or_default();
        }
        let ty = v.attr("type").unwrap_or("").to_lowercase();
        let asv = v.attr("as").unwrap_or("");
        if asv == "video" || ty.starts_with("video/") || ty.contains("mpegurl") {
            c.push(h, "link", "video", "", "");
        } else if asv == "audio" || ty.starts_with("audio/") {
            c.push(h, "link", "audio", "", "");
        }
    }
    for e in doc.select(&sel("video, audio")) {
        let v = e.value();
        let hint = if v.name() == "audio" { "audio" } else { "video" };
        let poster = v.attr("poster").and_then(|p| absolutize(&base, p)).unwrap_or_else(|| og_image.clone());
        let t = v.attr("title").or(v.attr("aria-label")).unwrap_or(&title).to_string();
        for a in ["src", "data-src", "data-video-src", "data-audio-src", "data-url"] {
            if let Some(s) = v.attr(a) {
                c.push(s, v.name(), hint, &t, &poster);
            }
        }
        for s in e.select(&sel("source[src], source[data-src]")) {
            let sv = s.value();
            let sh = sv.attr("type").map(|t| kind_of_mime(t)).filter(|k| !k.is_empty()).unwrap_or(hint);
            c.push(sv.attr("src").or(sv.attr("data-src")).unwrap(), v.name(), sh, &t, &poster);
        }
        for s in e.select(&sel("track[src]")) {
            c.push(s.value().attr("src").unwrap(), "track", "doc", &t, "");
        }
    }
    for e in doc.select(&sel("source[src], embed[src], object[data]")) {
        let v = e.value();
        let s = v.attr("src").or(v.attr("data")).unwrap_or("");
        let hint = v.attr("type").map(kind_of_mime).unwrap_or("");
        c.push(s, v.name(), hint, "", "");
    }
    for e in doc.select(&sel("iframe[src], iframe[data-src]")) {
        let v = e.value();
        let s = v.attr("src").or(v.attr("data-src")).unwrap_or("");
        if let Some(u) = absolutize(&base, s) {
            if is_media_host(&u) && c.seen.insert(u.clone()) {
                c.items.push(Item {
                    url: u.clone(),
                    kind: "embed".into(),
                    name: host_of(&u),
                    source: "iframe".into(),
                    title: v.attr("title").unwrap_or("").into(),
                    page: page_url.into(),
                    ..Default::default()
                });
            }
        }
    }
    let mut links = vec![];
    for a in doc.select(&sel("a[href]")) {
        let v = a.value();
        let h = v.attr("href").unwrap_or("");
        let text: String = a.text().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
        let Some(u) = absolutize(&base, h) else { continue };
        let e = url_ext(&u);
        if is_file_ext(&e) || STREAM.contains(&e.as_str()) {
            c.push(h, "a", "", &text, "");
        } else if v.attr("download").is_some() {
            c.push(h, "a", "other", &text, "");
            if let Some(it) = c.items.iter_mut().find(|i| i.url == u) {
                if it.kind == "other" {
                    it.kind = "unknown".into();
                }
            }
        } else if is_media_host(&u) && (u.contains("watch?v=") || u.contains("/shorts/") || u.contains("youtu.be/")) {
            if c.seen.insert(u.clone()) {
                c.items.push(Item { url: u.clone(), kind: "embed".into(), name: text.clone(), title: text, source: "a".into(), page: page_url.into(), ..Default::default() });
            }
        } else {
            links.push(u);
        }
    }
    for i in doc.select(&sel("img")) {
        let v = i.value();
        let alt = v.attr("alt").unwrap_or("");
        let mut cands: Vec<String> = vec![];
        for a in ["data-src", "data-original", "data-lazy-src", "data-full", "data-large", "src"] {
            if let Some(s) = v.attr(a) {
                cands.push(s.into());
            }
        }
        for a in ["srcset", "data-srcset"] {
            if let Some(s) = v.attr(a).and_then(best_srcset) {
                cands.insert(0, s);
            }
        }
        if let Some(s) = cands.into_iter().find(|s| !s.starts_with("data:")) {
            c.push(&s, "img", "image", alt, "");
        }
    }
    for s in doc.select(&sel(r#"script[type="application/ld+json"]"#)) {
        let txt: String = s.text().collect();
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
            walk_ld(&v, &mut c, &title);
        }
    }
    // ham metin de taranıyor, script içindeki kaçışlı adresler için
    let raw = html.replace("\\u002F", "/").replace("\\/", "/").replace("&amp;", "&");
    for cap in re_url().captures_iter(&raw) {
        c.push(&cap[1], "script", "", "", "");
    }
    Page { title: title.trim().into(), icon, items: c.items, links }
}

fn walk_ld(v: &serde_json::Value, c: &mut Collector, title: &str) {
    match v {
        serde_json::Value::Object(o) => {
            let name = o.get("name").and_then(|x| x.as_str()).unwrap_or(title).to_string();
            let thumb = match o.get("thumbnailUrl") {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(serde_json::Value::Array(a)) => a.first().and_then(|x| x.as_str()).unwrap_or("").into(),
                _ => String::new(),
            };
            for k in ["contentUrl", "embedUrl"] {
                if let Some(u) = o.get(k).and_then(|x| x.as_str()) {
                    let hint = match o.get("@type").and_then(|x| x.as_str()) {
                        Some("AudioObject") => "audio",
                        _ => "video",
                    };
                    if k == "embedUrl" && is_media_host(u) {
                        if c.seen.insert(u.to_string()) {
                            c.items.push(Item { url: u.into(), kind: "embed".into(), name: name.clone(), title: name.clone(), thumb: thumb.clone(), source: "ld+json".into(), page: c.page.clone(), ..Default::default() });
                        }
                    } else {
                        c.push(u, "ld+json", hint, &name, &thumb);
                    }
                }
            }
            for x in o.values() {
                walk_ld(x, c, title);
            }
        }
        serde_json::Value::Array(a) => {
            for x in a {
                walk_ld(x, c, title);
            }
        }
        _ => {}
    }
}

pub async fn fetch_page(url: &str, h: &BTreeMap<String, String>) -> Result<(String, String)> {
    let mut rb = client()
        .get(url)
        .timeout(Duration::from_secs(25))
        .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .header("Accept-Language", "tr-TR,tr;q=0.9,en;q=0.8");
    for (k, v) in h {
        rb = rb.header(k.as_str(), v.as_str());
    }
    let r = rb.send().await?;
    let fin = r.url().to_string();
    let ct = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_lowercase();
    if !ct.is_empty() && !ct.contains("html") && !ct.contains("xml") && !ct.contains("text") {
        anyhow::bail!("NOTHTML:{ct}");
    }
    let body = r.text().await?;
    Ok((fin, body))
}

/// boyut/tür yoklaması, HEAD olmazsa 0-0 aralıklı GET
/// site = çerezin ait olduğu host, başka hostlara çerez ve yetki başlığı gitmez
pub async fn enrich(items: &mut [Item], h: &BTreeMap<String, String>, site: &str) {
    let jobs: Vec<(usize, String)> = items
        .iter()
        .enumerate()
        .filter(|(_, i)| i.kind != "embed" && i.kind != "media")
        .take(500)
        .map(|(n, i)| (n, i.url.clone()))
        .collect();
    let res: Vec<(usize, u64, String)> = stream::iter(jobs)
        .map(|(n, u)| {
            let mut h = h.clone();
            if host_of(&u) != site {
                h.retain(|k, _| !k.eq_ignore_ascii_case("cookie") && !k.eq_ignore_ascii_case("authorization"));
            }
            async move {
                let mut rb = client().head(&u).timeout(Duration::from_secs(8));
                for (k, v) in &h {
                    rb = rb.header(k.as_str(), v.as_str());
                }
                let mut size = 0;
                let mut mime = String::new();
                if let Ok(r) = rb.send().await {
                    if r.status().is_success() {
                        size = r.content_length().unwrap_or(0);
                        mime = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").into();
                    }
                }
                if size == 0 {
                    let mut rb = client().get(&u).header("Range", "bytes=0-0").timeout(Duration::from_secs(8));
                    for (k, v) in &h {
                        rb = rb.header(k.as_str(), v.as_str());
                    }
                    if let Ok(r) = rb.send().await {
                        let cr = r.headers().get("content-range").and_then(|v| v.to_str().ok()).unwrap_or("");
                        size = cr.rsplit('/').next().and_then(|x| x.parse().ok()).unwrap_or(0);
                        if mime.is_empty() {
                            mime = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").into();
                        }
                    }
                }
                (n, size, mime.split(';').next().unwrap_or("").trim().to_lowercase())
            }
        })
        .buffer_unordered(24)
        .collect()
        .await;
    for (n, size, mime) in res {
        let it = &mut items[n];
        it.size = size;
        if !mime.is_empty() {
            let k = kind_of_mime(&mime);
            if !k.is_empty() && (it.kind == "unknown" || it.kind == "other" || it.ext.is_empty()) {
                it.kind = k.into();
            }
            if it.ext.is_empty() {
                let e = crate::http::mime_ext(&mime);
                if !e.is_empty() {
                    it.ext = e.into();
                    if crate::util::ext_of(&it.name).is_empty() {
                        it.name = format!("{}.{e}", if it.name.is_empty() { "dosya" } else { &it.name });
                    }
                }
            }
            it.mime = mime;
        }
    }
}

/// depth 0 sadece sayfa, 1-2 aynı sitedeki bağlı sayfalar da
pub async fn crawl<F: Fn(usize, usize) + Send + Sync>(
    url: &str,
    depth: u32,
    h: &BTreeMap<String, String>,
    progress: F,
) -> Result<ScrapeResult> {
    let (fin, body) = fetch_page(url, h).await?;
    let first = parse(&fin, &body);
    let mut res = ScrapeResult { url: fin.clone(), title: first.title.clone(), icon: first.icon.clone(), pages: 1, items: vec![] };
    let mut seen_items: HashSet<String> = HashSet::new();
    let mut all = vec![];
    for it in first.items {
        if seen_items.insert(it.url.clone()) {
            all.push(it);
        }
    }
    progress(1, all.len());
    if depth > 0 {
        let host = host_of(&fin);
        let max_pages = if depth == 1 { 40 } else { 120 };
        let mut visited: HashSet<String> = HashSet::from([fin.clone()]);
        let mut q: VecDeque<(String, u32)> = first.links.into_iter().map(|l| (l, 1)).collect();
        while !q.is_empty() && visited.len() < max_pages {
            let mut batch = vec![];
            while let Some((l, d)) = q.pop_front() {
                if host_of(&l) != host || !visited.insert(l.clone()) {
                    continue;
                }
                batch.push((l, d));
                if batch.len() >= 8 || visited.len() >= max_pages {
                    break;
                }
            }
            let results: Vec<_> = stream::iter(batch)
                .map(|(l, d)| {
                    let h = h.clone();
                    async move { (fetch_page(&l, &h).await, d) }
                })
                .buffer_unordered(8)
                .collect()
                .await;
            for (r, d) in results {
                let Ok((pu, b)) = r else { continue };
                let p = parse(&pu, &b);
                res.pages += 1;
                for it in p.items {
                    if seen_items.insert(it.url.clone()) {
                        all.push(it);
                    }
                }
                if d < depth {
                    q.extend(p.links.into_iter().map(|l| (l, d + 1)));
                }
            }
            progress(res.pages, all.len());
        }
    }
    enrich(&mut all, h, &host_of(&fin)).await;
    // önce medya, sonra büyük dosyalar
    let rank: HashMap<&str, i32> = [("embed", 0), ("stream", 1), ("video", 2), ("audio", 3), ("archive", 4), ("app", 5), ("doc", 6), ("unknown", 7), ("image", 8)].into_iter().collect();
    all.sort_by(|a, b| {
        rank.get(a.kind.as_str()).unwrap_or(&9).cmp(rank.get(b.kind.as_str()).unwrap_or(&9)).then(b.size.cmp(&a.size))
    });
    res.items = all;
    Ok(res)
}
