//! yt-dlp sarmalayıcı

use crate::manager::{Manager, STOPPED};
use crate::model::{Status, Task};
use crate::util::*;
use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

pub fn ytdlp() -> Result<PathBuf> {
    find_tool("yt-dlp").ok_or_else(|| anyhow!("TOOLS:yt-dlp bulunamadı. Ayarlar > Bileşenler'den kur"))
}

fn base_cmd() -> Result<Command> {
    let mut c = Command::new(ytdlp()?);
    c.creation_flags(NO_WINDOW)
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .args(["--no-colors", "--encoding", "utf-8"]);
    if let Some(ff) = find_tool("ffmpeg") {
        c.arg("--ffmpeg-location").arg(ff);
    }
    // youtube imzası için js runtime lazım, önce deno sonra node
    if let Some(d) = find_tool("deno") {
        c.arg("--js-runtimes").arg(format!("deno:{}", d.display()));
    } else if let Some(n) = find_tool("node") {
        c.arg("--js-runtimes").arg(format!("node:{}", n.display()));
    }
    Ok(c)
}

// eklentinin yakaladığı ham m3u8/mpd'de bütün başlıkları geçir, korumalı akış ancak öyle iniyor.
// yt-dlp'nin kendi çözücüsü olan sitelerde sadece Referer; youtube çerezle kısıtlı istemciye düşüp
// "Requested format is not available" veriyor.
fn add_headers(c: &mut Command, url: &str, h: &std::collections::BTreeMap<String, String>) {
    let site = is_media_host(url);
    for (k, v) in h {
        if hop_header(k) || v.is_empty() || v.chars().any(char::is_control) || (site && !k.eq_ignore_ascii_case("Referer")) {
            continue;
        }
        c.arg("--add-header").arg(format!("{k}:{v}"));
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct VideoOpt {
    pub height: u64,
    pub size: u64,
    pub fps: u64,
    pub hdr: bool,
    pub codec: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct Entry {
    pub url: String,
    pub title: String,
    pub duration: f64,
    pub thumb: String,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct MediaInfo {
    pub kind: String, // media | playlist
    pub url: String,
    pub title: String,
    pub thumb: String,
    pub duration: f64,
    pub uploader: String,
    pub extractor: String,
    pub videos: Vec<VideoOpt>,
    pub audio_abr: u64,
    pub audio_size: u64,
    pub has_video: bool,
    pub subs: Vec<String>,
    pub entries: Vec<Entry>,
    pub direct: bool,
    pub views: u64,
    pub upload_date: String,
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}
fn n(v: &Value, k: &str) -> f64 {
    v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0)
}

pub async fn analyze(url: &str, headers: &std::collections::BTreeMap<String, String>) -> Result<MediaInfo> {
    let mut c = base_cmd()?;
    c.args(["-J", "--flat-playlist", "--no-warnings", "--playlist-end", "2000"]);
    add_headers(&mut c, url, headers);
    c.arg("--").arg(url);
    let out = tokio::time::timeout(std::time::Duration::from_secs(90), c.output())
        .await
        .map_err(|_| anyhow!("Çözümleme zaman aşımına uğradı"))??;
    if !out.status.success() {
        let e = String::from_utf8_lossy(&out.stderr);
        let line = e.lines().rev().find(|l| l.contains("ERROR")).unwrap_or(e.trim());
        bail!("{}", clean_err(line));
    }
    let v: Value = serde_json::from_slice(&out.stdout)?;
    let mut info = MediaInfo {
        url: { let w = s(&v, "webpage_url"); if w.is_empty() { url.to_string() } else { w } },
        title: s(&v, "title"),
        uploader: { let u = s(&v, "uploader"); if u.is_empty() { s(&v, "channel") } else { u } },
        extractor: s(&v, "extractor_key"),
        duration: n(&v, "duration"),
        views: n(&v, "view_count") as u64,
        upload_date: s(&v, "upload_date"),
        direct: v.get("direct").and_then(|x| x.as_bool()).unwrap_or(false),
        ..Default::default()
    };
    if s(&v, "_type") == "playlist" || v.get("entries").is_some() {
        info.kind = "playlist".into();
        for e in v.get("entries").and_then(|x| x.as_array()).cloned().unwrap_or_default() {
            let mut u = s(&e, "url");
            if u.is_empty() {
                u = s(&e, "webpage_url");
            }
            if !u.starts_with("http") && s(&e, "ie_key") == "Youtube" {
                u = format!("https://www.youtube.com/watch?v={u}");
            }
            if u.is_empty() {
                continue;
            }
            let thumb = e
                .get("thumbnails")
                .and_then(|t| t.as_array())
                .and_then(|a| a.last())
                .map(|t| s(t, "url"))
                .unwrap_or_default();
            info.entries.push(Entry { url: u, title: s(&e, "title"), duration: n(&e, "duration"), thumb });
        }
        info.thumb = info.entries.first().map(|e| e.thumb.clone()).unwrap_or_default();
        if info.title.is_empty() {
            info.title = "Oynatma listesi".into();
        }
        return Ok(info);
    }
    info.kind = "media".into();
    info.thumb = s(&v, "thumbnail");
    let fmts = v.get("formats").and_then(|x| x.as_array()).cloned().unwrap_or_default();
    let none = |f: &Value, k: &str| matches!(f.get(k).and_then(|x| x.as_str()), None | Some("none"));
    let size = |f: &Value| n(f, "filesize").max(n(f, "filesize_approx")) as u64;
    let best_a = fmts.iter().filter(|f| !none(f, "acodec") && none(f, "vcodec")).max_by(|a, b| {
        n(a, "abr").partial_cmp(&n(b, "abr")).unwrap_or(std::cmp::Ordering::Equal)
    });
    info.audio_abr = best_a.map(|f| n(f, "abr").round() as u64).unwrap_or(0);
    info.audio_size = best_a.map(size).unwrap_or(0);
    let mut hm: std::collections::BTreeMap<u64, VideoOpt> = Default::default();
    for f in &fmts {
        let h = n(f, "height") as u64;
        if h == 0 || none(f, "vcodec") {
            continue;
        }
        let sz = size(f) + if none(f, "acodec") { info.audio_size } else { 0 };
        let fps = n(f, "fps") as u64;
        let hdr = !matches!(f.get("dynamic_range").and_then(|x| x.as_str()), None | Some("SDR"));
        let codec = s(f, "vcodec").split('.').next().unwrap_or("").to_string();
        let e = hm.entry(h).or_insert(VideoOpt { height: h, size: 0, fps: 0, hdr: false, codec: codec.clone() });
        if sz > e.size {
            e.size = sz;
            e.codec = codec;
        }
        e.fps = e.fps.max(fps);
        e.hdr |= hdr;
    }
    info.videos = hm.into_values().rev().collect();
    info.has_video = !info.videos.is_empty();
    if let Some(o) = v.get("subtitles").and_then(|x| x.as_object()) {
        info.subs = o.keys().filter(|k| *k != "live_chat").take(60).cloned().collect();
    }
    Ok(info)
}

pub async fn run(t: Arc<Task>, m: Arc<Manager>) -> Result<()> {
    let (url, folder, o, headers) = {
        let mut d = t.d.lock();
        d.status = Status::Downloading;
        (d.url.clone(), d.folder.clone(), d.opts.clone(), d.headers.clone())
    };
    std::fs::create_dir_all(&folder)?;
    let mut c = base_cmd()?;
    let tpl = PathBuf::from(&folder).join("%(title).170B.%(ext)s");
    c.args(["--newline", "--progress", "--no-playlist", "--windows-filenames", "--no-mtime", "--continue"])
        .args(["--concurrent-fragments", "8", "--retries", "15", "--fragment-retries", "15"])
        .args(["--progress-template", "download:VDL %(progress)j"])
        .args(["--progress-template", "postprocess:VPP %(progress.postprocessor)s|%(progress.status)s"])
        .args(["--print", "after_move:VFILE %(filepath)s"])
        .args(["--print", "before_dl:VTITLE %(title)s"])
        .arg("-o")
        .arg(&tpl);
    add_headers(&mut c, &url, &headers);
    // yt-dlp ortak sınırı bilmiyor, çalışan medya işleri arasında paylaştır
    let running = m.tasks.lock().iter().filter(|x| x.d.lock().kind == crate::model::Kind::Media && x.running.load(Ordering::SeqCst)).count().max(1) as u64;
    let lim = m.settings.lock().speed_limit / running;
    if m.settings.lock().speed_limit > 0 {
        c.args(["--limit-rate", &format!("{}K", lim.max(1))]);
    }
    if o.mode == "audio" {
        let codec = if o.codec.is_empty() { "mp3".to_string() } else { o.codec.clone() };
        c.args(["-f", "ba/b", "-x", "--audio-format", &codec])
            .args(["--audio-quality", &format!("{}K", if o.bitrate == 0 { 320 } else { o.bitrate })])
            .args(["--embed-metadata"]);
        if matches!(codec.as_str(), "mp3" | "m4a" | "flac" | "opus") {
            c.args(["--embed-thumbnail", "--convert-thumbnails", "jpg"]);
        }
        t.d.lock().stage = "Ses".into();
    } else {
        let h = o.height;
        let hf = if h > 0 { format!("[height<={h}]") } else { String::new() };
        let cont = if o.container.is_empty() { "mp4".to_string() } else { o.container.clone() };
        let mut sort = if h > 0 { format!("res:{h}") } else { "res".into() };
        if cont == "mp4" {
            // aynı çözünürlükte h264 + aac, her oynatıcı açıyor
            sort.push_str(",vcodec:h264,acodec:m4a");
        }
        c.args(["-f", &format!("bv*{hf}+ba/b{hf}/bv*+ba/b"), "-S", &sort, "--merge-output-format", &cont])
            .args(["--embed-metadata"]);
        if o.chapters {
            c.arg("--embed-chapters");
        }
        if !o.subs.is_empty() {
            c.args(["--embed-subs", "--sub-langs", &o.subs.join(",")]);
        }
        t.d.lock().stage = "Video".into();
    }
    if o.sponsorblock {
        c.args(["--sponsorblock-remove", "sponsor,selfpromo,interaction"]);
    }
    c.arg("--").arg(&url);
    let mut ch = c.spawn()?;
    let job = ProcJob::new();
    if let Some(j) = &job {
        j.add(&ch);
    }
    let out = ch.stdout.take().unwrap();
    let err = ch.stderr.take().unwrap();
    let errbuf = Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));
    let eb = errbuf.clone();
    let et = tokio::spawn(async move {
        let mut r = BufReader::new(err).lines();
        while let Ok(Some(l)) = r.next_line().await {
            let mut g = eb.lock();
            g.push(l);
            if g.len() > 40 {
                g.remove(0);
            }
        }
    });
    let tok = t.token();
    let mut lines = BufReader::new(out).lines();
    let (mut base, mut cur_total, mut cur_file) = (0u64, 0u64, String::new());
    let mut streams = 0;
    loop {
        let l = tokio::select! {
            _ = tok.cancelled() => {
                if let Some(j) = &job {
                    j.kill();
                }
                let _ = ch.kill().await;
                bail!(STOPPED)
            }
            l = lines.next_line() => l?,
        };
        let Some(l) = l else { break };
        if let Some(j) = l.strip_prefix("VDL ") {
            let Ok(p) = serde_json::from_str::<Value>(j) else { continue };
            let fname = s(&p, "filename");
            if fname != cur_file {
                if !cur_file.is_empty() {
                    base += cur_total;
                }
                streams += 1;
                if streams == 2 && o.mode != "audio" {
                    t.d.lock().stage = "Ses".into();
                }
                cur_file = fname;
            }
            let total = n(&p, "total_bytes").max(n(&p, "total_bytes_estimate")) as u64;
            let got = n(&p, "downloaded_bytes") as u64;
            cur_total = total.max(got);
            t.d.lock().size = base + cur_total;
            t.set_done(base + got);
            let frag = p.get("fragment_count").is_some();
            t.conns.store(if frag { 8 } else { 1 }, Ordering::Relaxed);
            if s(&p, "status") == "finished" {
                t.set_done(base + cur_total);
            }
        } else if let Some(pp) = l.strip_prefix("VPP ") {
            let (name, st) = pp.split_once('|').unwrap_or((pp, ""));
            if st == "started" {
                t.conns.store(0, Ordering::Relaxed);
                let label = match name {
                    x if x.contains("Merger") => "Birleştiriliyor",
                    x if x.contains("ExtractAudio") => "Ses dönüştürülüyor",
                    x if x.contains("EmbedThumbnail") => "Kapak ekleniyor",
                    x if x.contains("Metadata") => "Etiketler yazılıyor",
                    x if x.contains("EmbedSubtitle") => "Altyazı gömülüyor",
                    x if x.contains("SponsorBlock") || x.contains("ModifyChapters") => "Reklamlar kesiliyor",
                    x if x.contains("MoveFiles") => "Taşınıyor",
                    _ => "İşleniyor",
                };
                let mut d = t.d.lock();
                d.status = Status::Processing;
                d.stage = label.into();
            }
        } else if let Some(f) = l.strip_prefix("VFILE ") {
            let mut d = t.d.lock();
            d.path = f.trim().to_string();
            d.name = PathBuf::from(&d.path).file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
        } else if let Some(ti) = l.strip_prefix("VTITLE ") {
            let mut d = t.d.lock();
            if d.title.is_empty() {
                d.title = ti.trim().to_string();
            }
        }
    }
    let st = ch.wait().await?;
    let _ = et.await;
    if tok.is_cancelled() {
        bail!(STOPPED);
    }
    if !st.success() {
        let g = errbuf.lock();
        let line = g.iter().rev().find(|l| l.contains("ERROR")).cloned().unwrap_or_else(|| g.last().cloned().unwrap_or_default());
        bail!("{}", clean_err(&line));
    }
    let mut d = t.d.lock();
    if let Ok(md) = std::fs::metadata(&d.path) {
        d.size = md.len();
        t.set_done(md.len());
    }
    if !d.name.is_empty() {
        let c = category_of(&d.name);
        if c != "diger" {
            d.category = c;
        }
    }
    Ok(())
}

/// iptal edilen medyadan kalan .part dosyalarını siler
pub fn cleanup(folder: &str, title: &str) {
    if title.is_empty() {
        return;
    }
    let Ok(rd) = std::fs::read_dir(folder) else { return };
    let key: String = safe_name(title).chars().take(40).collect();
    for e in rd.flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        if n.starts_with(&key) && (n.ends_with(".part") || n.contains(".part-Frag") || n.ends_with(".ytdl")) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}
