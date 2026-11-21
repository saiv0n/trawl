//! çok bağlantılı, kaldığı yerden devam eden http indirici.
//! boşa çıkan bağlantı en büyük kalan parçanın yarısını kapıyor.

use crate::manager::{Limiter, Manager};
use crate::model::{Seg, Status, Task};
use crate::util::*;
use anyhow::{anyhow, bail, Result};
use futures_util::StreamExt;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::os::windows::fs::FileExt;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

const MIN_SPLIT: u64 = 1 << 20;

type HostLimits = parking_lot::Mutex<std::collections::HashMap<String, (u32, std::time::Instant)>>;

// pixeldrain gibi siteler anonime tek bağlantı veriyor, host başına hatırla.
// sınır kalkmış olabilir, yarım saat sonra yeniden dene
fn host_limits() -> &'static HostLimits {
    static L: std::sync::OnceLock<HostLimits> = std::sync::OnceLock::new();
    L.get_or_init(Default::default)
}

/// aynı sunucuya bütün indirmelerin toplamı, ayarlardan geliyor
pub static HOST_CONNS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(16);

fn host_used() -> &'static parking_lot::Mutex<std::collections::HashMap<String, u32>> {
    static U: std::sync::OnceLock<parking_lot::Mutex<std::collections::HashMap<String, u32>>> = std::sync::OnceLock::new();
    U.get_or_init(Default::default)
}

/// sunucudan pay alır, düşünce geri verir. en az bir bağlantı her zaman var
pub(crate) struct HostShare {
    host: String,
    pub(crate) n: u32,
}

impl HostShare {
    pub(crate) fn take(host: String, want: u32) -> Self {
        let total = HOST_CONNS.load(Ordering::Relaxed);
        let mut u = host_used().lock();
        let used = u.entry(host.clone()).or_insert(0);
        let n = if total == 0 { want } else { want.min(total.saturating_sub(*used)).max(1) };
        *used += n;
        Self { host, n }
    }
}

impl Drop for HostShare {
    fn drop(&mut self) {
        let mut u = host_used().lock();
        if let Some(x) = u.get_mut(&self.host) {
            *x = x.saturating_sub(self.n);
            if *x == 0 {
                u.remove(&self.host);
            }
        }
    }
}

fn host_cap(host: &str) -> u32 {
    let mut l = host_limits().lock();
    match l.get(host) {
        Some((n, at)) if at.elapsed() < Duration::from_secs(30 * 60) => *n,
        Some(_) => {
            l.remove(host);
            u32::MAX
        }
        None => u32::MAX,
    }
}

#[derive(Debug, PartialEq)]
enum Refusal {
    /// sınıra takıldı, parçasını diğerleri alır
    Limit { explicit: bool },
    /// adres ölmüş (süresi dolmuş, silinmiş)
    Dead,
}

/// others = şu an açık diğer bağlantılar
fn classify(code: u16, body: &str, others: u32) -> Option<Refusal> {
    let b = body.to_lowercase();
    let says_limit = ["concurrent", "too many", "connection limit", "max_conn", "maximum number of", "rate limit", "slow down"].iter().any(|k| b.contains(k));
    match code {
        429 | 503 => Some(Refusal::Limit { explicit: true }),
        401 | 403 | 404 | 410 if says_limit => Some(Refusal::Limit { explicit: true }),
        // diğerleri aynı adresten inerken 403 geldiyse adres canlı, sınır bize
        401 | 403 if others > 0 => Some(Refusal::Limit { explicit: false }),
        401 | 403 | 404 | 410 => Some(Refusal::Dead),
        _ => None,
    }
}
const FLUSH: usize = 512 * 1024;

#[derive(Serialize, Clone, Debug, Default)]
pub struct Probe {
    pub url: String,
    pub size: u64,
    pub resumable: bool,
    pub name: String,
    pub ctype: String,
    /// güçlü ETag, yoksa Last-Modified
    pub validator: String,
}

/// :body varsa aynı gövdeyle POST at
pub fn rq(url: &str, h: &BTreeMap<String, String>) -> reqwest::RequestBuilder {
    let rb = match h.get(BODY_KEY) {
        Some(b) => client().post(url).body(b.clone()),
        None => client().get(url),
    };
    let rb = hdrs(rb, h);
    // sayfadan gelen yetki başlığı varsa ona dokunma
    match login_for(url) {
        Some((u, p)) if hget(h, "Authorization").is_none() => rb.basic_auth(u, Some(p)),
        _ => rb,
    }
}

pub fn hdrs(rb: reqwest::RequestBuilder, h: &BTreeMap<String, String>) -> reqwest::RequestBuilder {
    let mut rb = rb.header("Accept-Encoding", "identity");
    if hget(h, "User-Agent").is_none() {
        let ua = browser_ua();
        if !ua.is_empty() {
            rb = rb.header("User-Agent", ua);
        }
    }
    for (k, v) in h {
        if !hop_header(k) {
            rb = rb.header(k.as_str(), v.as_str());
        }
    }
    rb
}

pub async fn probe(url: &str, h: &BTreeMap<String, String>) -> Result<Probe> {
    let r = rq(url, h)
        .header("Range", "bytes=0-0")
        .timeout(Duration::from_secs(25))
        .send()
        .await?;
    let st = r.status();
    if st.as_u16() >= 400 {
        bail!("Sunucu {} döndü", st.as_u16());
    }
    let hd = r.headers();
    let get = |k: &str| hd.get(k).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let mut size = 0;
    let mut resumable = false;
    let cr = get("content-range");
    if st.as_u16() == 206 {
        if let Some(t) = cr.rsplit('/').next() {
            if let Ok(n) = t.trim().parse::<u64>() {
                size = n;
                resumable = true;
            }
        }
    } else if let Ok(n) = get("content-length").parse::<u64>() {
        size = n;
    }
    let etag = get("etag");
    let validator = if !etag.is_empty() && !etag.starts_with("W/") { etag } else { get("last-modified") };
    let ctype = get("content-type").split(';').next().unwrap_or("").trim().to_lowercase();
    let cd = disposition(hd.get("content-disposition").map(|v| v.as_bytes()).unwrap_or_default());
    let mut name = String::new();
    if let Some(c) = regex::Regex::new(r#"(?i)filename\*\s*=\s*[^']*'[^']*'([^;]+)"#).unwrap().captures(&cd) {
        name = percent_encoding::percent_decode_str(c[1].trim_matches(|c| c == '"' || c == ' '))
            .decode_utf8_lossy()
            .to_string();
    } else if let Some(c) = regex::Regex::new(r#"(?i)filename\s*=\s*"?([^";]+)"?"#).unwrap().captures(&cd) {
        name = c[1].trim().to_string();
    }
    let final_url = r.url().to_string();
    if name.is_empty() {
        name = name_from_url(&final_url);
    }
    if ext_of(&name).is_empty() {
        let e = mime_ext(&ctype);
        if !e.is_empty() {
            name = format!("{}.{e}", if name.is_empty() { "dosya" } else { &name });
        }
    }
    Ok(Probe { url: final_url, size, resumable, name: safe_name(&name), ctype, validator })
}

/// php sitelerinin çoğu dosya adını ham utf-8 ya da latin-1 yolluyor, to_str() bunu atıyordu
fn disposition(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => b.iter().map(|&c| c as char).collect(),
    }
}

pub fn mime_ext(m: &str) -> &'static str {
    match m {
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/x-matroska" => "mkv",
        "video/quicktime" => "mov",
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/mp4" | "audio/x-m4a" => "m4a",
        "audio/ogg" => "ogg",
        "audio/wav" | "audio/x-wav" => "wav",
        "audio/flac" => "flac",
        "audio/aac" => "aac",
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "application/pdf" => "pdf",
        "application/zip" | "application/x-zip-compressed" => "zip",
        "application/x-rar-compressed" | "application/vnd.rar" => "rar",
        "application/x-7z-compressed" => "7z",
        "application/x-msdownload" | "application/vnd.microsoft.portable-executable" => "exe",
        "application/x-msi" => "msi",
        "application/vnd.android.package-archive" => "apk",
        "application/x-iso9660-image" => "iso",
        "text/plain" => "txt",
        _ => "",
    }
}

pub async fn run(t: Arc<Task>, m: Arc<Manager>) -> Result<()> {
    let conns = m.settings.lock().connections;
    run_with(t, m.limiter.clone(), conns).await
}

/// tauri'ye bağlı değil, testte doğrudan kurulabiliyor
pub async fn run_with(t: Arc<Task>, lim: Arc<Limiter>, conns: u32) -> Result<()> {
    let (need_probe, url, headers) = {
        let d = t.d.lock();
        (d.size == 0 || d.segs.is_empty(), d.url.clone(), d.headers.clone())
    };
    if need_probe {
        t.d.lock().status = Status::Probing;
        let p = probe(&url, &headers).await?;
        if p.ctype.contains("text/html") && url_ext(&url).is_empty() {
            bail!("Bu bağlantı bir dosya değil, bir web sayfası");
        }
        let mut d = t.d.lock();
        if d.src.is_empty() && p.url != d.url {
            d.src = d.url.clone();
        }
        d.url = p.url;
        d.size = p.size;
        d.resumable = p.resumable;
        d.validator = p.validator;
        if d.name.is_empty() {
            d.name = p.name;
        }
        d.category = category_of(&d.name);
    }
    let path = {
        let mut d = t.d.lock();
        if d.path.is_empty() {
            let p = unique_path(&PathBuf::from(&d.folder).join(&d.name));
            d.name = p.file_name().unwrap().to_string_lossy().to_string();
            d.path = p.to_string_lossy().to_string();
        }
        d.status = Status::Downloading;
        PathBuf::from(&d.path)
    };
    let part = PathBuf::from(format!("{}.trawl", path.display()));
    if let Some(dir) = part.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let (size, resumable) = {
        let d = t.d.lock();
        (d.size, d.resumable)
    };
    if size > 0 {
        let have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        let need = if resumable { size.saturating_sub(t.get_done()) } else { size };
        let need = if have >= size { need } else { need.max(size - have) };
        if let Some(free) = free_space(part.parent().unwrap_or(std::path::Path::new("."))) {
            if free < need + (32 << 20) {
                let drive = part.to_string_lossy().chars().next().unwrap_or('?');
                bail!("DISK:{drive}:{free}:{need}");
            }
        }
    }
    if size > 0 && resumable {
        multi(&t, &lim, conns, &part).await?;
    } else {
        single(&t, &lim, &part).await?;
    }
    if t.token().is_cancelled() {
        bail!(crate::manager::STOPPED);
    }
    let mut fin = path.clone();
    if fin.exists() {
        fin = unique_path(&fin);
        let mut d = t.d.lock();
        d.path = fin.to_string_lossy().to_string();
        d.name = fin.file_name().unwrap().to_string_lossy().to_string();
    }
    std::fs::rename(&part, &fin)?;
    Ok(())
}

async fn single(t: &Arc<Task>, lim: &Arc<Limiter>, part: &PathBuf) -> Result<()> {
    let (url, h) = {
        let mut d = t.d.lock();
        d.segs.clear();
        (d.url.clone(), d.headers.clone())
    };
    t.set_done(0);
    t.conns.store(1, Ordering::Relaxed);
    let tok = t.token();
    let r = tokio::time::timeout(Duration::from_secs(30), rq(&url, &h).send())
        .await
        .map_err(|_| anyhow!("Sunucu yanıt vermiyor (zaman aşımı)"))??
        .error_for_status()?;
    let expect = match t.d.lock().size {
        0 => r.content_length().unwrap_or(0),
        n => n,
    };
    let f = std::fs::File::create(part)?;
    let mut off = 0u64;
    let mut st = r.bytes_stream();
    loop {
        let c = tokio::select! {
            _ = tok.cancelled() => break,
            c = tokio::time::timeout(Duration::from_secs(60), st.next()) => c,
        };
        let Ok(c) = c else { bail!("Sunucu yanıt vermiyor (zaman aşımı)") };
        let Some(c) = c else { break };
        let c = c?;
        f.seek_write(&c, off)?;
        off += c.len() as u64;
        t.add_done(c.len() as u64);
        tokio::select! { _ = tok.cancelled() => {}, _ = take2(lim, &t.lim, c.len() as u64) => {} }
    }
    t.conns.store(0, Ordering::Relaxed);
    if !tok.is_cancelled() && expect > 0 && off < expect {
        bail!("İndirme eksik kaldı, sunucu bağlantıyı erken kesti");
    }
    let mut d = t.d.lock();
    if d.size == 0 {
        d.size = off;
    }
    Ok(())
}

async fn multi(t: &Arc<Task>, lim: &Arc<Limiter>, conns: u32, part: &PathBuf) -> Result<()> {
    let want = {
        let d = t.d.lock();
        if d.conns_max > 0 { d.conns_max } else { conns }
    }
    .clamp(1, 64);
    let host = host_of(&t.d.lock().url);
    let cap = host_cap(&host);
    let share = HostShare::take(host, want.min(cap).max(1));
    let want = share.n as u64;
    let fresh = !part.exists() || t.d.lock().segs.is_empty();
    let file = if fresh {
        let f = OpenOptions::new().create(true).write(true).truncate(true).read(true).open(part)?;
        let size = t.d.lock().size;
        // seyrek değilse windows sondan ilk yazışta aradaki her yeri sıfırla doldurup bekletiyor
        set_sparse(&f);
        f.set_len(size)?;
        let n = want.min(size.div_ceil(MIN_SPLIT)).max(1);
        let step = size / n;
        let mut d = t.d.lock();
        d.segs = (0..n)
            .map(|i| Seg { s: i * step, e: if i == n - 1 { size - 1 } else { (i + 1) * step - 1 }, p: 0, busy: false })
            .collect();
        f
    } else {
        OpenOptions::new().write(true).read(true).open(part)?
    };
    {
        let mut d = t.d.lock();
        for s in d.segs.iter_mut() {
            s.busy = false;
        }
        t.set_done(d.segs.iter().map(|s| s.p).sum());
    }
    t.last_refusal.store(0, Ordering::Relaxed);
    let file = Arc::new(file);
    *t.file.lock() = Some(file.clone());
    let err: Arc<parking_lot::Mutex<Option<anyhow::Error>>> = Arc::new(parking_lot::Mutex::new(None));
    let mut hs = vec![];
    for i in 0..want {
        let (t, lim, f, err) = (t.clone(), lim.clone(), file.clone(), err.clone());
        hs.push(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(i * 150)).await;
            if let Err(e) = worker(&t, &lim, &f).await {
                let mut g = err.lock();
                if g.is_none() && !t.token().is_cancelled() {
                    *g = Some(e);
                }
                t.token().cancel();
            }
        }));
    }
    for h in hs {
        let _ = h.await;
    }
    *t.file.lock() = None;
    if let Some(e) = err.lock().take() {
        return Err(e);
    }
    if !t.token().is_cancelled() && t.d.lock().segs.iter().any(|s| s.left() > 0) {
        // hepsi 403 yiyip çekildiyse adres ölmüş, tazelemeye bırak
        let code = t.last_refusal.load(Ordering::Relaxed);
        if code != 0 {
            bail!("Bağlantının süresi dolmuş olabilir ({code})");
        }
        bail!("İndirme eksik kaldı");
    }
    Ok(())
}

/// boşta parça yoksa en büyüğü böl
fn next_seg(t: &Task) -> Option<usize> {
    let mut d = t.d.lock();
    if let Some(i) = d.segs.iter().position(|s| !s.busy && s.left() > 0) {
        d.segs[i].busy = true;
        return Some(i);
    }
    let (bi, rem) = d
        .segs
        .iter()
        .enumerate()
        .filter(|(_, s)| s.busy)
        .map(|(i, s)| (i, s.left()))
        .max_by_key(|x| x.1)?;
    if rem < 3 * MIN_SPLIT {
        return None;
    }
    let mid = d.segs[bi].pos() + rem / 2;
    let e = d.segs[bi].e;
    d.segs[bi].e = mid - 1;
    d.segs.push(Seg { s: mid, e, p: 0, busy: true });
    Some(d.segs.len() - 1)
}

async fn worker(t: &Arc<Task>, lim: &Arc<Limiter>, f: &Arc<std::fs::File>) -> Result<()> {
    let tok = t.token();
    while !tok.is_cancelled() {
        let Some(i) = next_seg(t) else { return Ok(()) };
        t.conns.fetch_add(1, Ordering::Relaxed);
        let r = fetch(t, lim, f, i).await;
        t.conns.fetch_sub(1, Ordering::Relaxed);
        t.d.lock().segs[i].busy = false;
        match r {
            // sunucu yoğun, parçayı diğerleri alır
            Err(e) if e.to_string() == "SHRINK" => return Ok(()),
            r => r?,
        }
    }
    Ok(())
}

/// parça bittiyse ya da bölündüyse true
fn flush(t: &Task, f: &std::fs::File, i: usize, buf: &mut Vec<u8>) -> Result<(bool, u64)> {
    let (pos, room) = {
        let d = t.d.lock();
        (d.segs[i].pos(), d.segs[i].left())
    };
    let n = (buf.len() as u64).min(room) as usize;
    if n > 0 {
        f.seek_write(&buf[..n], pos)?;
        t.d.lock().segs[i].p += n as u64;
        t.add_done(n as u64);
    }
    let full = n < buf.len() || n as u64 == room;
    buf.clear();
    Ok((full, n as u64))
}

async fn fetch(t: &Arc<Task>, lim: &Arc<Limiter>, f: &std::fs::File, i: usize) -> Result<()> {
    let tok = t.token();
    let (url, h, validator) = {
        let d = t.d.lock();
        (d.url.clone(), d.headers.clone(), d.validator.clone())
    };
    let mut tries = 0u32;
    let mut buf: Vec<u8> = Vec::with_capacity(FLUSH + 65536);
    while !tok.is_cancelled() {
        let (start, end) = {
            let d = t.d.lock();
            (d.segs[i].pos(), d.segs[i].e)
        };
        if start > end {
            return Ok(());
        }
        let res: Result<bool> = async {
            let mut rb = rq(&url, &h).header("Range", format!("bytes={start}-{end}"));
            if !validator.is_empty() {
                rb = rb.header("If-Range", validator.as_str());
            }
            // bazı sunucular fazla bağlantıyı reddetmek yerine cevapsız bekletiyor
            let r = match tokio::time::timeout(Duration::from_secs(20), rb.send()).await {
                Ok(r) => r?,
                Err(_) => {
                    let others = t.conns.load(Ordering::Relaxed).saturating_sub(1);
                    if others > 0 {
                        let mut l = host_limits().lock();
                        let e = l.entry(host_of(&url)).or_insert((u32::MAX, std::time::Instant::now()));
                        *e = (e.0.min(others), std::time::Instant::now());
                        return Err(anyhow!("SHRINK"));
                    }
                    return Err(anyhow!("zaman aşımı"));
                }
            };
            let code = r.status().as_u16();
            // If-Range tutmazsa sunucu bütün dosyayı 200 ile yollar, eski parçalarla karışmasın
            let echoed = ["etag", "last-modified"].iter().any(|k| r.headers().get(*k).and_then(|v| v.to_str().ok()) == Some(validator.as_str()));
            if code == 200 && !validator.is_empty() && !echoed {
                return Err(anyhow!("FATAL:Sunucudaki dosya değişmiş. Baştan indir ile yeniden başlatın"));
            }
            if code == 200 && start != 0 {
                return Err(anyhow!("FATAL:Sunucu parçalı indirmeyi reddetti"));
            }
            if code >= 400 {
                let body: String = r.text().await.unwrap_or_default().chars().take(2000).collect();
                let others = t.conns.load(Ordering::Relaxed).saturating_sub(1);
                match classify(code, &body, others) {
                    Some(Refusal::Limit { explicit }) => {
                        if matches!(code, 401 | 403) {
                            t.last_refusal.store(code, Ordering::Relaxed);
                        }
                        if others > 0 {
                            if explicit {
                                // sunucu sınırı söyledi, host için hatırla
                                let mut l = host_limits().lock();
                                let e = l.entry(host_of(&url)).or_insert((u32::MAX, std::time::Instant::now()));
                                *e = (e.0.min(others.max(1)), std::time::Instant::now());
                            }
                            return Err(anyhow!("SHRINK"));
                        }
                        // tek bağlantı kaldıysa bekle, tekrar dene
                        return Err(anyhow!("BUSY"));
                    }
                    Some(Refusal::Dead) => return Err(anyhow!("FATAL:Bağlantının süresi dolmuş olabilir ({code})")),
                    None => return Err(anyhow!("Sunucu {code} döndü")),
                }
            }
            let mut st = r.bytes_stream();
            loop {
                let c = tokio::select! {
                    _ = tok.cancelled() => { flush(t, f, i, &mut buf)?; return Ok(true) }
                    c = tokio::time::timeout(Duration::from_secs(30), st.next()) => c,
                };
                let c = match c {
                    Err(_) => return Err(anyhow!("zaman aşımı")),
                    Ok(None) => {
                        flush(t, f, i, &mut buf)?;
                        return Ok(t.d.lock().segs[i].left() == 0);
                    }
                    Ok(Some(c)) => c?,
                };
                let n = c.len() as u64;
                buf.extend_from_slice(&c);
                if buf.len() >= FLUSH {
                    let (full, _) = flush(t, f, i, &mut buf)?;
                    if full {
                        return Ok(true);
                    }
                }
                tokio::select! { _ = tok.cancelled() => {}, _ = take2(lim, &t.lim, n) => {} }
                tries = 0;
            }
        }
        .await;
        match res {
            Ok(true) => return Ok(()),
            Ok(false) => {
                tries += 1;
                if tries > 10 {
                    bail!("Sunucu bağlantıyı erken kesiyor");
                }
            }
            Err(e) => {
                let _ = flush(t, f, i, &mut buf);
                let s = e.to_string();
                if let Some(x) = s.strip_prefix("FATAL:") {
                    bail!("{x}");
                }
                if s == "SHRINK" {
                    bail!("SHRINK");
                }
                // ağ gittiyse sınırsız bekle, geri gelince kaldığı yerden sürsün
                let net = e.downcast_ref::<reqwest::Error>().is_some_and(|r| !r.is_builder()) || s == "zaman aşımı";
                tries = if net { (tries + 1).min(6) } else { tries + 1 };
                if tries > 10 {
                    if s == "BUSY" {
                        bail!("Sunucu bağlantı sınırında ya da yoğun. Bu siteden başka bir indirme sürüyorsa onun bitmesini bekleyip yeniden deneyin");
                    }
                    return Err(e);
                }
            }
        }
        tokio::select! {
            _ = tok.cancelled() => {}
            _ = tokio::time::sleep(Duration::from_millis((400 * 2u64.pow(tries.min(6))).min(20_000))) => {}
        }
    }
    Ok(())
}

async fn take2(a: &Limiter, b: &Limiter, n: u64) {
    a.take(n).await;
    b.take(n).await;
}

fn set_sparse(f: &std::fs::File) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Ioctl::FSCTL_SET_SPARSE;
    use windows_sys::Win32::System::IO::DeviceIoControl;
    let mut n = 0u32;
    // fat32/exfat desteklemiyor, olmazsa eskisi gibi devam
    unsafe {
        DeviceIoControl(f.as_raw_handle() as _, FSCTL_SET_SPARSE, std::ptr::null(), 0, std::ptr::null_mut(), 0, &mut n, std::ptr::null_mut());
    }
}

pub fn free_space(dir: &std::path::Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let mut p = dir.to_path_buf();
    while !p.exists() {
        p = p.parent()?.to_path_buf();
    }
    let wide: Vec<u16> = p.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut avail: u64 = 0;
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, std::ptr::null_mut(), std::ptr::null_mut()) };
    if ok != 0 { Some(avail) } else { None }
}

#[cfg(test)]
mod tests {
    use super::{classify, Refusal};

    /// gerçek pixeldrain'e 8 bağlantıyla gir, 7'si 403 yer; indirme tek bağlantıyla sürüp doğru bitmeli
    #[tokio::test]
    #[ignore = "ağ gerektirir"]
    async fn pixeldrain_single_connection_limit() {
        use crate::model::{Task, TaskData};
        use std::sync::atomic::Ordering;
        use std::sync::Arc;
        let url = "https://pixeldrain.com/api/file/pZMhNTcz?download";
        let dir = std::env::temp_dir().join(format!("trawl-pd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let t = Arc::new(Task::new(TaskData { url: url.into(), folder: dir.to_string_lossy().into(), conns_max: 8, ..Default::default() }));
        let run = tokio::spawn(super::run_with(t.clone(), Arc::new(crate::manager::Limiter::new()), 8));
        let mut max_conns = 0;
        for _ in 0..40 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            max_conns = max_conns.max(t.conns.load(Ordering::Relaxed));
            assert!(!run.is_finished(), "indirme erken bitti: {:?}", t.d.lock().error);
        }
        let (done, conns, path) = (t.get_done(), t.conns.load(Ordering::Relaxed), t.d.lock().path.clone());
        t.token().cancel();
        let _ = run.await;
        let limit = super::host_limits().lock().get("pixeldrain.com").map(|x| x.0);
        // baştan inen kısım sunucudakiyle aynı mı
        let seg0 = t.d.lock().segs.iter().find(|s| s.s == 0).map(|s| s.p).unwrap_or(0).min(1 << 20);
        let local = std::fs::read(format!("{path}.trawl")).unwrap_or_default();
        let remote = if seg0 > 0 {
            reqwest::Client::new().get(url).header("Range", format!("bytes=0-{}", seg0 - 1)).send().await.unwrap().bytes().await.unwrap().to_vec()
        } else {
            vec![]
        };
        let _ = std::fs::remove_dir_all(&dir);
        eprintln!("inen {done} B, en çok {max_conns} bağlantı, sonda {conns}, hatırlanan sınır {limit:?}, doğrulanan {seg0} B");
        assert!(done > 2 << 20, "yalnızca {done} B indi");
        // pixeldrain sınırı zamanla değiştiriyor (önce 1, sonra 4), sabit sayı beklemiyoruz
        assert!(limit.is_some_and(|l| (1..8).contains(&l)), "sınır öğrenilmedi: {limit:?}");
        assert!(conns >= 1 && conns <= limit.unwrap(), "sınırdan fazla bağlantı: {conns}");
        assert!(seg0 > 0 && local[..seg0 as usize] == remote[..], "baytlar uyuşmuyor");
    }

    #[test]
    fn connection_limit_is_not_an_expired_link() {
        // pixeldrain anonimde ikinci bağlantıya açıklamalı 403
        let pd = r#"{"success":false,"value":"max_concurrent_downloads","message":"You have reached the maximum number of open download connections"}"#;
        assert_eq!(classify(403, pd, 1), Some(Refusal::Limit { explicit: true }));
        assert_eq!(classify(403, pd, 0), Some(Refusal::Limit { explicit: true }));
        // açıklamasız 403 ama diğerleri iniyor, demek sınır
        assert_eq!(classify(403, "Forbidden", 3), Some(Refusal::Limit { explicit: false }));
        // tek bağlantıda 403/410 ise adres ölü, tazeleme
        assert_eq!(classify(403, "<html>AccessDenied</html>", 0), Some(Refusal::Dead));
        assert_eq!(classify(410, "", 4), Some(Refusal::Dead));
        assert_eq!(classify(429, "", 0), Some(Refusal::Limit { explicit: true }));
        assert_eq!(classify(500, "", 2), None);
    }

    #[test]
    fn post_body_becomes_post_and_is_not_a_header() {
        let mut h = std::collections::BTreeMap::new();
        h.insert(crate::util::BODY_KEY.to_string(), "a=1&b=2".to_string());
        h.insert("Content-Type".to_string(), "application/x-www-form-urlencoded".to_string());
        h.insert("Sec-Fetch-Mode".to_string(), "navigate".to_string());
        let r = super::rq("https://x.test/dl", &h).build().unwrap();
        assert_eq!(r.method(), "POST");
        assert_eq!(r.body().and_then(|b| b.as_bytes()), Some(&b"a=1&b=2"[..]));
        assert!(r.headers().keys().all(|k| !k.as_str().starts_with(':') && !k.as_str().starts_with("sec-")));
        assert_eq!(r.headers()["content-type"], "application/x-www-form-urlencoded");
        h.remove(crate::util::BODY_KEY);
        assert_eq!(super::rq("https://x.test/dl", &h).build().unwrap().method(), "GET");
    }

    #[tokio::test]
    #[ignore = "ağ gerektirir"]
    async fn probe_hetzner() {
        let r = super::probe("https://fsn1-speed.hetzner.com/100MB.bin", &Default::default()).await;
        match r {
            Ok(p) => println!("OK {:?}", p),
            Err(e) => println!("ERR {e:#}"),
        }
    }
}
