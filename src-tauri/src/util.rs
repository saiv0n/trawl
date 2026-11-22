use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[cfg(windows)]
pub const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Trawl/1.0";
#[cfg(not(windows))]
pub const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) Trawl/1.0";

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
    // CON, NUL, COM1 gibi adlar uzantılı olsa da açılmıyor
    let stem = s.split('.').next().unwrap_or("").trim().to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        s.insert(0, '_');
    }
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

fn proxy_cell() -> &'static parking_lot::RwLock<String> {
    static P: OnceLock<parking_lot::RwLock<String>> = OnceLock::new();
    P.get_or_init(Default::default)
}

pub fn set_proxy(p: &str) {
    *proxy_cell().write() = p.trim().to_string();
}

pub fn proxy() -> String {
    proxy_cell().read().clone()
}

/// proxy değişince yeniden kurulur, Client ucuz kopyalanıyor
pub fn client() -> reqwest::Client {
    static C: OnceLock<parking_lot::Mutex<Option<(String, reqwest::Client)>>> = OnceLock::new();
    let p = proxy();
    let mut g = C.get_or_init(Default::default).lock();
    if let Some((k, c)) = g.as_ref() {
        if *k == p {
            return c.clone();
        }
    }
    let c = build_client(&p);
    *g = Some((p, c.clone()));
    c
}

fn build_client(p: &str) -> reqwest::Client {
    let mut b = reqwest::Client::builder();
    if !p.is_empty() {
        if let Ok(x) = reqwest::Proxy::all(p) {
            b = b.proxy(x);
        }
    }
    b.user_agent(UA)
        .connect_timeout(std::time::Duration::from_secs(15))
        .pool_max_idle_per_host(64)
        .tcp_nodelay(true)
        .http1_only()
        .build()
        .expect("http istemcisi")
}

/// uzunluğa göre md5, sha1 ya da sha256; dosyayı parça parça okur
pub fn hash_file(path: &str, expected: &str) -> Result<(String, String), String> {
    use sha2::Digest;
    use std::io::Read;
    let want = expected.trim().to_lowercase();
    let mut f = std::fs::File::open(path).map_err(|_| "Dosya bulunamadı".to_string())?;
    let mut buf = vec![0u8; 1 << 20];
    macro_rules! run {
        ($h:expr, $name:expr) => {{
            let mut h = $h;
            loop {
                let n = f.read(&mut buf).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            let got: String = h.finalize().iter().map(|x| format!("{x:02x}")).collect();
            Ok(($name.to_string(), got))
        }};
    }
    match want.len() {
        32 => run!(md5::Md5::new(), "MD5"),
        40 => run!(sha1::Sha1::new(), "SHA-1"),
        64 => run!(sha2::Sha256::new(), "SHA-256"),
        _ => Err("MD5, SHA-1 ya da SHA-256 değeri yapıştırın".into()),
    }
}

/// exe'nin yanında "portable" dosyası varsa her şey exe'nin yanındaki data klasöründe
pub fn portable_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    dir.join("portable").exists().then(|| dir.join("data"))
}

pub fn data_dir() -> PathBuf {
    if let Some(p) = portable_dir() {
        let _ = std::fs::create_dir_all(&p);
        return p;
    }
    #[cfg(windows)]
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    #[cfg(not(windows))]
    let base = std::env::var("XDG_DATA_HOME").ok().filter(|v| v.starts_with('/')).unwrap_or_else(|| format!("{}/.local/share", std::env::var("HOME").unwrap_or_else(|_| ".".into())));
    let p = PathBuf::from(&base).join("Trawl");
    // eski adla (Velora) kalan veriyi bir kez taşı
    let old = PathBuf::from(&base).join(["Vel", "ora"].concat());
    if !p.exists() && old.exists() {
        let _ = std::fs::rename(&old, &p);
    }
    let _ = std::fs::create_dir_all(&p);
    // çerezler ve şifreler burada, linux'ta başka kullanıcılar okumasın
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700));
    }
    p
}

pub fn bin_dir() -> PathBuf {
    let p = data_dir().join("bin");
    let _ = std::fs::create_dir_all(&p);
    p
}

/// önce uygulama verisi, sonra exe'nin yanı, en son PATH
pub fn find_tool(name: &str) -> Option<PathBuf> {
    let exe = format!("{name}{}", std::env::consts::EXE_SUFFIX);
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

#[cfg(windows)]
pub const NO_WINDOW: u32 = 0x0800_0000;

/// windows'ta konsol penceresi açmasın, linux'ta kendi süreç grubunda dursun ki torunlarıyla birlikte öldürülebilsin
pub trait NoWindow {
    fn no_window(&mut self) -> &mut Self;
}
impl NoWindow for std::process::Command {
    fn no_window(&mut self) -> &mut Self {
        #[cfg(windows)]
        std::os::windows::process::CommandExt::creation_flags(self, NO_WINDOW);
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(self, 0);
        self
    }
}
impl NoWindow for tokio::process::Command {
    fn no_window(&mut self) -> &mut Self {
        #[cfg(windows)]
        self.creation_flags(NO_WINDOW);
        #[cfg(unix)]
        self.process_group(0);
        self
    }
}

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
            let need = p[2].split_whitespace().next().unwrap_or("0").parse::<u64>().unwrap_or(0);
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
    if low.contains("cloudflare") || low.contains("impersonat") || low.contains("anti-bot") {
        return "Site bot korumasına takıldı (Cloudflare gibi). Sayfayı tarayıcıda açıp eklentinin panelinden indirmeyi deneyin".into();
    }
    if low.contains("login required") || low.contains("use --cookies") || low.contains("logged-in") || low.contains("requires authentication") {
        return "Bu içerik oturum açmayı istiyor. Sayfayı tarayıcıda açıp eklentiyle indirin ya da Ayarlar > Video ve ses'ten tarayıcı çerezlerini seçin".into();
    }
    if low.contains("drm") {
        return "Bu video DRM ile korunuyor, indirilemez".into();
    }
    if low.contains("failed to decrypt") || low.contains("app-bound") {
        return "Chrome ve Edge çerezlerini şifreliyor, okunamıyor. Ayarlar > Video ve ses'ten Firefox'u seçin ya da videoyu sayfadayken eklentiyle indirin".into();
    }
    if low.contains("cookie database") || low.contains("could not find") && low.contains("cookies") {
        return "Tarayıcının çerezleri okunamadı. Tarayıcı açıkken çerezlerini kilitliyor; kapatıp yeniden deneyin".into();
    }
    if low.contains("sign in to confirm") {
        return "Site oturum açma istiyor (yaş ya da bot doğrulaması). Ayarlar > Video'dan tarayıcı çerezlerini kullanmayı açın".into();
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
    if let Some(c) = regex::Regex::new(r"(?:Sunucu |HTTP Error )(\d{3})").unwrap().captures(&m) {
        match &c[1] {
            "401" => return "Sunucu kullanıcı adı ve şifre istiyor. Ayarlar > Klasör ve giriş'ten bu site için giriş ekleyin".into(),
            "403" => return "Sunucu erişimi reddetti (403). Bağlantının süresi dolmuş olabilir; sayfadan yeni bağlantıyı alıp Bağlantıyı güncelle ile sürdürün".into(),
            "404" | "410" => return "Dosya sunucuda bulunamadı. Kaldırılmış ya da taşınmış olabilir.".into(),
            "416" => return "Sunucu istenen bölümü veremedi. Baştan indir ile yeniden deneyin".into(),
            "429" => return "Sunucu çok fazla istek dedi. Biraz bekleyip yeniden deneyin ya da bağlantı sayısını azaltın".into(),
            "500" | "502" | "503" | "504" => return "Sunucu şu an hata veriyor. Biraz sonra yeniden deneyin".into(),
            _ => {}
        }
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

#[cfg(windows)]
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

// linux'ta dpapi yok; veri klasörü zaten 0700. windows'tan taşınan mühürlü değerler açılamaz, atılır
#[cfg(not(windows))]
fn dpapi(_: &[u8], _: bool) -> Option<Vec<u8>> {
    None
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

fn logins() -> &'static parking_lot::RwLock<Vec<crate::model::Login>> {
    static L: std::sync::OnceLock<parking_lot::RwLock<Vec<crate::model::Login>>> = std::sync::OnceLock::new();
    L.get_or_init(Default::default)
}

pub fn set_logins(v: &[crate::model::Login]) {
    *logins().write() = v.iter().filter(|l| !l.host.trim().is_empty() && !l.user.is_empty()).cloned().collect();
}

/// bu adrese kayıtlı kullanıcı adı ve şifre
pub fn login_for(url: &str) -> Option<(String, String)> {
    logins().read().iter().find(|l| crate::model::host_match(&l.host, url)).map(|l| (l.user.clone(), l.pass.clone()))
}

pub fn seal_str(v: &str) -> String {
    if v.is_empty() || v.starts_with(SEALED) {
        return v.to_string();
    }
    // dpapi her seferinde farklı çıktı veriyor, önbelleksiz kayıt hiç "değişmedi" olmaz
    static C: std::sync::OnceLock<parking_lot::Mutex<std::collections::HashMap<String, String>>> = std::sync::OnceLock::new();
    let mut c = C.get_or_init(Default::default).lock();
    if let Some(x) = c.get(v) {
        return x.clone();
    }
    let x = dpapi(v.as_bytes(), true).map(|b| format!("{SEALED}{}", b.iter().map(|x| format!("{x:02x}")).collect::<String>())).unwrap_or_default();
    c.insert(v.to_string(), x.clone());
    x
}

pub fn unseal_str(v: &str) -> String {
    let Some(hex) = v.strip_prefix(SEALED) else { return v.to_string() };
    let bytes: Option<Vec<u8>> = (0..hex.len()).step_by(2).map(|i| hex.get(i..i + 2).and_then(|x| u8::from_str_radix(x, 16).ok())).collect();
    bytes.and_then(|b| dpapi(&b, false)).and_then(|b| String::from_utf8(b).ok()).unwrap_or_default()
}

#[cfg(all(test, windows))]
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
#[cfg(windows)]
pub struct ProcJob(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
unsafe impl Send for ProcJob {}
#[cfg(windows)]
unsafe impl Sync for ProcJob {}

#[cfg(windows)]
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

#[cfg(windows)]
impl Drop for ProcJob {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}

/// linux'ta süreç grubu; çocuk no_window() ile kendi grubunu açıyor, grubu öldürmek torunları da alıyor
#[cfg(unix)]
pub struct ProcJob(parking_lot::Mutex<Vec<i32>>);

#[cfg(unix)]
impl ProcJob {
    pub fn new() -> Option<Self> {
        Some(Self(parking_lot::Mutex::new(Vec::new())))
    }
    pub fn add(&self, c: &tokio::process::Child) {
        if let Some(p) = c.id() {
            self.0.lock().push(p as i32);
        }
    }
    pub fn kill(&self) {
        for &g in self.0.lock().iter() {
            unsafe { libc::killpg(g, libc::SIGKILL) };
        }
    }
}

#[cfg(unix)]
impl Drop for ProcJob {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(all(test, windows))]
mod job_tests {
    use super::NoWindow;
    #[tokio::test]
    async fn job_kill_takes_grandchildren() {
        // cmd'nin açtığı ping torun süreç; iş nesnesi ikisini birden kapatmalı
        let mut ch = tokio::process::Command::new("cmd").args(["/c", "ping -n 30 127.0.0.1 >nul"]).no_window().spawn().unwrap();
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

#[cfg(all(test, unix))]
mod job_tests_unix {
    use super::NoWindow;
    #[tokio::test]
    async fn job_kill_takes_grandchildren() {
        let mut ch = tokio::process::Command::new("sh").args(["-c", "sleep 30 & wait"]).no_window().spawn().unwrap();
        let job = super::ProcJob::new().unwrap();
        job.add(&ch);
        let pg = ch.id().unwrap() as i32;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        job.kill();
        let _ = ch.wait().await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        // grupta kimse kalmadıysa killpg ESRCH döner
        assert_eq!(unsafe { libc::killpg(pg, 0) }, -1);
    }
}

#[cfg(test)]
mod hash_tests {
    #[test]
    fn picks_algo_by_length() {
        let p = std::env::temp_dir().join(format!("trawl-hash-{}", std::process::id()));
        std::fs::write(&p, b"abc").unwrap();
        let p = p.to_string_lossy().to_string();
        assert_eq!(super::hash_file(&p, "900150983CD24FB0D6963F7D28E17F72").unwrap().1, "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(super::hash_file(&p, &"0".repeat(64)).unwrap().1, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(super::hash_file(&p, "a9993e364706816aba3e25717850c26c9cd0d89d").unwrap().0, "SHA-1");
        assert!(super::hash_file(&p, "kısa").is_err());
        let _ = std::fs::remove_file(&p);
    }
}
