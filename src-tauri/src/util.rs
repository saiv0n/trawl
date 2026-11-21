use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Trawl/1.0";

// eklentinin son ping'teki gerçek UA'sı; UA'ya bağlı imzalı CDN'ler için
fn browser_ua_cell() -> &'static parking_lot::RwLock<String> {
    static C: OnceLock<parking_lot::RwLock<String>> = OnceLock::new();
    C.get_or_init(|| parking_lot::RwLock::new(String::new()))
}
pub fn set_browser_ua(ua: &str) {
    if ua.starts_with("Mozilla/") && *browser_ua_cell().read() != ua {
        *browser_ua_cell().write() = ua.to_string();
    }
}
pub fn browser_ua() -> String {
    browser_ua_cell().read().clone()
}

/// bağlantıya özel, taşınmayacak başlıklar
pub fn hop_header(k: &str) -> bool {
    let k = k.to_ascii_lowercase();
    k.starts_with(':') || k.starts_with("sec-") || matches!(k.as_str(), "host" | "range" | "accept-encoding" | "content-length" | "connection" | "if-range" | "if-none-match" | "if-modified-since" | "upgrade-insecure-requests")
}

// POST gövdesi başlık haritasında bu sahte anahtarla saklanıyor, istekte gönderilmez
pub const BODY_KEY: &str = ":body";

pub fn hget<'a>(h: &'a std::collections::BTreeMap<String, String>, k: &str) -> Option<&'a String> {
    h.iter().find(|(x, _)| x.eq_ignore_ascii_case(k)).map(|(_, v)| v)
}

/// harf farkıyla aynı adlı eskiyi silip yazar, çift Referer gitmesin
pub fn hput(h: &mut std::collections::BTreeMap<String, String>, k: &str, v: String) {
    h.retain(|x, _| !x.eq_ignore_ascii_case(k));
    h.insert(k.to_string(), v);
}

pub const CATS: &[(&str, &[&str])] = &[
    ("video", &["mp4", "mkv", "webm", "avi", "mov", "wmv", "flv", "m4v", "ts", "3gp", "mpg", "mpeg", "ogv"]),
    ("muzik", &["mp3", "m4a", "aac", "flac", "wav", "ogg", "opus", "wma", "alac", "aiff", "mid"]),
    ("belge", &["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "epub", "csv", "odt", "rtf", "mobi", "djvu"]),
    ("program", &["exe", "msi", "apk", "dmg", "deb", "rpm", "appimage", "msix", "jar", "xapk"]),
    ("arsiv", &["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "iso", "img", "zst", "tgz", "cab"]),
    ("resim", &["jpg", "jpeg", "png", "gif", "webp", "bmp", "svg", "tiff", "avif", "heic", "ico", "psd"]),
];

pub const MEDIA_HOSTS: &[&str] = &[
    "youtube.com", "youtu.be", "soundcloud.com", "vimeo.com", "tiktok.com", "instagram.com", "twitter.com",
    "x.com", "twitch.tv", "dailymotion.com", "reddit.com", "facebook.com", "fb.watch", "bilibili.com",
    "bandcamp.com", "mixcloud.com", "kick.com", "pinterest.com", "vk.com", "rumble.com", "streamable.com",
    "music.youtube.com", "threads.net", "bsky.app", "9gag.com", "imgur.com", "odysee.com", "nicovideo.jp",
];

pub fn cat_dir(c: &str) -> &'static str {
    match c {
        "video" => "Video",
        "muzik" => "Müzik",
        "belge" => "Belgeler",
        "program" => "Programlar",
        "arsiv" => "Arşivler",
        "resim" => "Resimler",
        _ => "Diğer",
    }
}

pub fn ext_of(name: &str) -> String {
    Path::new(name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn category_of(name: &str) -> String {
    let e = ext_of(name);
    for (c, exts) in CATS {
        if exts.contains(&e.as_str()) {
            return c.to_string();
        }
    }
    "diger".into()
}

pub fn is_file_ext(e: &str) -> bool {
    CATS.iter().any(|(_, x)| x.contains(&e))
}

pub fn url_ext(u: &str) -> String {
    url::Url::parse(u).map(|x| ext_of(x.path())).unwrap_or_default()
}

pub fn host_of(u: &str) -> String {
    url::Url::parse(u).ok().and_then(|x| x.host_str().map(|h| h.to_lowercase())).unwrap_or_default()
}

pub fn is_media_host(u: &str) -> bool {
    let h = host_of(u);
    MEDIA_HOSTS.iter().any(|m| h == *m || h.ends_with(&format!(".{m}")))
}

pub fn safe_name(n: &str) -> String {
    let mut s: String = n
        .chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || (c as u32) < 32 { '_' } else { c })
        .collect();
    s = s.trim().trim_matches('.').trim().to_string();
    if s.chars().count() > 200 {
        let ext = ext_of(&s);
        let stem: String = s.chars().take(190).collect();
        s = if ext.is_empty() { stem } else { format!("{stem}.{ext}") };
    }
    if s.is_empty() { "dosya".into() } else { s }
}

pub fn name_from_url(u: &str) -> String {
    let p = url::Url::parse(u).map(|x| x.path().to_string()).unwrap_or_default();
    let last = p.rsplit('/').next().unwrap_or("");
    percent_encoding::percent_decode_str(last).decode_utf8_lossy().to_string()
}

pub fn unique_path(p: &Path) -> PathBuf {
    let part = |x: &Path| PathBuf::from(format!("{}.trawl", x.display()));
    if !p.exists() && !part(p).exists() {
        return p.to_path_buf();
    }
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = p.extension().map(|s| format!(".{}", s.to_string_lossy())).unwrap_or_default();
    let dir = p.parent().unwrap_or(Path::new("."));
    for i in 1.. {
        let c = dir.join(format!("{stem} ({i}){ext}"));
        if !c.exists() && !part(&c).exists() {
            return c;
        }
    }
    unreachable!()
}

pub fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(UA)
            .connect_timeout(std::time::Duration::from_secs(15))
            .pool_max_idle_per_host(64)
            .tcp_nodelay(true)
            .http1_only()
            .build()
            .expect("http istemcisi")
    })
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    let p = PathBuf::from(&base).join("Trawl");
    // eski adla (Velora) kalan veriyi bir kez taşı
    let old = PathBuf::from(&base).join(["Vel", "ora"].concat());
    if !p.exists() && old.exists() {
        let _ = std::fs::rename(&old, &p);
    }
    let _ = std::fs::create_dir_all(&p);
    p
}

pub fn bin_dir() -> PathBuf {
    let p = data_dir().join("bin");
    let _ = std::fs::create_dir_all(&p);
    p
}

/// önce uygulama verisi, sonra exe'nin yanı, en son PATH
pub fn find_tool(name: &str) -> Option<PathBuf> {
    let exe = format!("{name}.exe");
    let mut c = vec![bin_dir().join(&exe)];
    if let Ok(me) = std::env::current_exe() {
        if let Some(d) = me.parent() {
            c.push(d.join(&exe));
            c.push(d.join("bin").join(&exe));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for d in std::env::split_paths(&path) {
            c.push(d.join(&exe));
        }
    }
    c.into_iter().find(|p| p.is_file())
}

pub const NO_WINDOW: u32 = 0x0800_0000;

fn human(n: u64) -> String {
    let u = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < 4 {
        v /= 1024.0;
        i += 1;
    }
    format!("{} {}", format!("{:.1}", v).replace('.', ","), u[i])
}

pub fn clean_err(m: &str) -> String {
    if let Some(rest) = m.split("DISK:").nth(1) {
        let p: Vec<&str> = rest.split(':').collect();
        if p.len() >= 3 {
            let free = p[1].trim().parse::<u64>().unwrap_or(0);
            let need = p[2].trim().split_whitespace().next().unwrap_or("0").parse::<u64>().unwrap_or(0);
            return format!("Diskte yer kalmadı ({}: · {} boş, {} gerekli). Yer açın ya da başka klasör seçin.", p[0], human(free), human(need));
        }
    }
    let re = regex::Regex::new(r"^ERROR:\s*(\[[^\]]+\]\s*)?").unwrap();
    let m = re.replace(m.trim(), "").to_string();
    let low = m.to_lowercase();
    if low.contains("unsupported url") {
        return "Bu adres desteklenmiyor".into();
    }
    if low.contains("video unavailable") {
        return "Video kullanılamıyor".into();
    }
    if low.contains("private video") {
        return "Bu video gizli".into();
    }
    if low.contains("sign in to confirm") {
        return "Site oturum açma istiyor (yaş ya da bot doğrulaması)".into();
    }
    if low.contains("peer closed connection") || low.contains("connection reset") {
        return "Sunucu bağlantıyı kesti (koruma ya da sınır). Bağlantı sayısını azaltıp yeniden dene".into();
    }
    if low.contains("dns error") || low.contains("no such host") {
        return "Sunucu bulunamadı. İnternet bağlantını kontrol et".into();
    }
    if low.contains("timed out") || low.contains("zaman aşımı") {
        return "Sunucu yanıt vermiyor (zaman aşımı)".into();
    }
    if low.contains("http error 403") || low.contains("süresi dolmuş") {
        return "Bağlantının süresi dolmuş. Sayfayı yenileyip bağlantıyı yeniden kopyalayın, sonra Bağlantıyı güncelle'yi kullanın; indirme kaldığı yerden sürer.".into();
    }
    if low.contains("(404)") || low.contains("http error 404") {
        return "Dosya sunucuda bulunamadı. Kaldırılmış ya da taşınmış olabilir.".into();
    }
    if low.contains("access is denied") || low.contains("erişim engellendi") || low.contains("permission denied") {
        return "Bu klasöre yazılamıyor. Ayarlardan başka bir klasör seçin.".into();
    }
    if low.contains("parçalı indirmeyi reddetti") {
        return "Sunucu parçalı indirmeyi reddetti. Baştan indir ile tek bağlantıyla yeniden deneyin.".into();
    }
    m.chars().take(300).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn disk_message() {
        let m = super::clean_err("bağlam: DISK:C:125829120:2254857830");
        assert!(m.starts_with("Diskte yer kalmadı (C: · 120,0 MB boş, 2,1 GB gerekli)"), "{m}");
    }
}

const SEALED: &str = "dpapi:";

fn sensitive(k: &str) -> bool {
    k.eq_ignore_ascii_case("cookie") || k.eq_ignore_ascii_case("authorization")
}

fn dpapi(data: &[u8], seal: bool) -> Option<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::*;
    let inp = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe {
        if seal {
            CryptProtectData(&inp, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        } else {
            CryptUnprotectData(&inp, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        }
    };
    if ok == 0 {
        return None;
    }
    let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec() };
    unsafe { LocalFree(out.pbData.cast()) };
    Some(v)
}

/// çerez ve yetki başlıkları diske kullanıcıya bağlı şifreli gider
pub fn seal_headers(h: &mut std::collections::BTreeMap<String, String>) {
    for (k, v) in h.iter_mut() {
        if sensitive(k) && !v.starts_with(SEALED) {
            if let Some(b) = dpapi(v.as_bytes(), true) {
                *v = format!("{SEALED}{}", b.iter().map(|x| format!("{x:02x}")).collect::<String>());
            }
        }
    }
}

pub fn unseal_headers(h: &mut std::collections::BTreeMap<String, String>) {
    h.retain(|_, v| {
        let Some(hex) = v.strip_prefix(SEALED) else { return true };
        let bytes: Option<Vec<u8>> = (0..hex.len()).step_by(2).map(|i| hex.get(i..i + 2).and_then(|x| u8::from_str_radix(x, 16).ok())).collect();
        match bytes.and_then(|b| dpapi(&b, false)).and_then(|b| String::from_utf8(b).ok()) {
            Some(s) => {
                *v = s;
                true
            }
            // başka kullanıcının ya da makinenin kaydı, açılamıyorsa at
            None => false,
        }
    });
}

#[cfg(test)]
mod seal_tests {
    #[test]
    fn cookie_roundtrip() {
        let mut h = std::collections::BTreeMap::from([("Cookie".to_string(), "s=1; t=2".to_string()), ("Referer".to_string(), "https://a.test/".to_string())]);
        super::seal_headers(&mut h);
        assert!(h["Cookie"].starts_with("dpapi:") && !h["Cookie"].contains("s=1"));
        assert_eq!(h["Referer"], "https://a.test/");
        super::unseal_headers(&mut h);
        assert_eq!(h["Cookie"], "s=1; t=2");
    }
}

/// alt süreçleri (yt-dlp'nin açtığı ffmpeg dahil) birlikte öldürmek için iş nesnesi.
/// tutamaç kapanınca, uygulama çökse bile, içindekiler de kapanır
pub struct ProcJob(windows_sys::Win32::Foundation::HANDLE);
unsafe impl Send for ProcJob {}
unsafe impl Sync for ProcJob {}

impl ProcJob {
    pub fn new() -> Option<Self> {
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if h.is_null() {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let p = (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast();
            SetInformationJobObject(h, JobObjectExtendedLimitInformation, p, std::mem::size_of_val(&info) as u32);
            Some(Self(h))
        }
    }
    pub fn add(&self, c: &tokio::process::Child) {
        if let Some(p) = c.raw_handle() {
            unsafe { windows_sys::Win32::System::JobObjects::AssignProcessToJobObject(self.0, p as _) };
        }
    }
    pub fn kill(&self) {
        unsafe { windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0, 1) };
    }
}

impl Drop for ProcJob {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}

#[cfg(test)]
mod job_tests {
    #[tokio::test]
    async fn job_kill_takes_grandchildren() {
        // cmd'nin açtığı ping torun süreç; iş nesnesi ikisini birden kapatmalı
        let mut ch = tokio::process::Command::new("cmd").args(["/c", "ping -n 30 127.0.0.1 >nul"]).creation_flags(super::NO_WINDOW).spawn().unwrap();
        let job = super::ProcJob::new().unwrap();
        job.add(&ch);
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let t = std::time::Instant::now();
        job.kill();
        let _ = ch.wait().await;
        assert!(t.elapsed().as_secs() < 3);
        let out = std::process::Command::new("tasklist").args(["/fi", &format!("PID eq {}", ch.id().unwrap_or(0))]).output().unwrap();
        assert!(!String::from_utf8_lossy(&out.stdout).contains("cmd.exe"));
    }
}
