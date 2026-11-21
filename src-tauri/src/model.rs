use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Http,
    Media,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Queued,
    Probing,
    Downloading,
    Processing,
    Paused,
    Done,
    Error,
}

impl Status {
    pub fn active(self) -> bool {
        matches!(self, Status::Probing | Status::Downloading | Status::Processing)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Seg {
    pub s: u64,
    pub e: u64,
    pub p: u64,
    #[serde(skip)]
    pub busy: bool,
}

impl Seg {
    pub fn pos(&self) -> u64 {
        self.s + self.p
    }
    pub fn left(&self) -> u64 {
        (self.e + 1).saturating_sub(self.pos())
    }
}

/// yt-dlp seçenekleri
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MediaOpts {
    pub mode: String,      // video | audio
    pub height: u32,       // 0 en iyisi
    pub container: String, // mp4 | mkv | webm
    pub codec: String,     // mp3 | m4a | opus | flac | wav
    pub bitrate: u32,
    pub subs: Vec<String>,
    pub chapters: bool,
    pub sponsorblock: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskData {
    pub id: String,
    pub kind: Kind,
    pub url: String,
    pub page: String,
    pub name: String,
    pub title: String,
    pub folder: String,
    pub path: String,
    pub size: u64,
    pub done: u64,
    pub status: Status,
    pub error: String,
    pub category: String,
    pub thumb: String,
    pub created: i64,
    pub finished: i64,
    pub headers: BTreeMap<String, String>,
    pub resumable: bool,
    pub segs: Vec<Seg>,
    pub opts: MediaOpts,
    pub conns_max: u32,
    pub stage: String,
    pub was_active: bool,
    /// otomatik tazeleme denendi mi, döngüye girmesin
    pub refresh_tries: u32,
    /// kaldığı yerden sürerken dosya değişti mi diye If-Range'e giden doğrulayıcı
    pub validator: String,
}

impl Default for TaskData {
    fn default() -> Self {
        Self {
            id: String::new(),
            kind: Kind::Http,
            url: String::new(),
            page: String::new(),
            name: String::new(),
            title: String::new(),
            folder: String::new(),
            path: String::new(),
            size: 0,
            done: 0,
            status: Status::Queued,
            error: String::new(),
            category: "diger".into(),
            thumb: String::new(),
            created: now(),
            finished: 0,
            headers: BTreeMap::new(),
            resumable: true,
            segs: vec![],
            opts: MediaOpts::default(),
            conns_max: 0,
            stage: String::new(),
            was_active: false,
            refresh_tries: 0,
            validator: String::new(),
        }
    }
}

/// zaman tabanlı EMA ile hız ve ETA (gösterim τ=2 sn, ETA τ=8 sn), firefox'taki gibi histerezisli
#[derive(Default)]
pub struct Est {
    pub v2: f64,
    pub v8: f64,
    pub last_eta: Option<f64>,
    pub since: f64,
    pub start_done: u64,
    pub stalled: f64,
    /// -1 bilinmiyor, -2 takıldı, >=0 saniye
    pub disp: f64,
}

impl Est {
    pub fn update(&mut self, inst: f64, dt: f64, done: u64, size: u64) {
        let a2 = 1.0 - (-dt / 2.0).exp();
        let a8 = 1.0 - (-dt / 8.0).exp();
        if self.since == 0.0 {
            self.v2 = inst;
            self.v8 = inst;
            self.start_done = done;
        } else {
            self.v2 += a2 * (inst - self.v2);
            self.v8 += a8 * (inst - self.v8);
        }
        self.since += dt;
        // takılmayı ham hızla ölç, yumuşatılmış hız sönerken eşiği geç geçiyor
        if inst < 1024.0 {
            self.stalled += dt;
        } else {
            if self.stalled > 5.0 {
                self.last_eta = None;
            }
            self.stalled = 0.0;
        }
        self.disp = -1.0;
        if size == 0 || size <= done {
            return;
        }
        if self.stalled > 5.0 && self.since > 5.0 {
            self.disp = -2.0;
            return;
        }
        let progressed = done.saturating_sub(self.start_done);
        let warmed = self.since >= 3.0 && (progressed >= 1 << 20 || progressed.saturating_mul(100) >= size);
        if !warmed || self.v8 < 1.0 {
            return;
        }
        let t = (size - done) as f64 / self.v8;
        let e = match self.last_eta {
            None => t,
            Some(l) if t < l / 2.0 => t,
            Some(l) => {
                let d = t - l;
                if d.abs() < 5f64.max(0.05 * l) {
                    l - dt
                } else {
                    l + if d < 0.0 { 0.3 } else { 0.1 } * d
                }
            }
        }
        .max(0.0);
        self.last_eta = Some(e);
        self.disp = e;
    }
}

pub struct Task {
    /// bağlantıyı geri çeviren son 401/403; hepsi çekilirse süresi dolmuş sayıyoruz
    pub last_refusal: std::sync::atomic::AtomicU16,
    pub d: Mutex<TaskData>,
    pub done: AtomicU64,
    pub conns: AtomicU32,
    pub last: AtomicU64,
    pub speed: Mutex<f64>,
    pub est: Mutex<Est>,
    pub cancel: Mutex<CancellationToken>,
    pub running: AtomicBool,
    pub removing: AtomicBool,
}

impl Task {
    pub fn new(d: TaskData) -> Self {
        let done = d.done;
        Self {
            d: Mutex::new(d),
            done: AtomicU64::new(done),
            conns: AtomicU32::new(0),
            last: AtomicU64::new(done),
            speed: Mutex::new(0.0),
            est: Mutex::new(Est::default()),
            cancel: Mutex::new(CancellationToken::new()),
            running: AtomicBool::new(false),
            removing: AtomicBool::new(false),
            last_refusal: std::sync::atomic::AtomicU16::new(0),
        }
    }
    pub fn id(&self) -> String {
        self.d.lock().id.clone()
    }
    pub fn status(&self) -> Status {
        self.d.lock().status
    }
    pub fn token(&self) -> CancellationToken {
        self.cancel.lock().clone()
    }
    pub fn add_done(&self, n: u64) {
        self.done.fetch_add(n, Ordering::Relaxed);
    }
    pub fn set_done(&self, n: u64) {
        self.done.store(n, Ordering::Relaxed);
    }
    pub fn get_done(&self) -> u64 {
        self.done.load(Ordering::Relaxed)
    }

    pub fn view(&self) -> TaskView {
        let d = self.d.lock();
        let done = self.get_done();
        let speed = *self.speed.lock();
        let eta = if d.status == Status::Downloading { self.est.lock().disp } else { -1.0 };
        let mut segs = vec![];
        if d.size > 0 && d.status != Status::Done {
            let mut v: Vec<&Seg> = d.segs.iter().collect();
            v.sort_by_key(|s| s.s);
            for s in v.into_iter().take(128) {
                let len = (s.e + 1).saturating_sub(s.s).max(1);
                segs.push([
                    s.s as f32 / d.size as f32,
                    (s.e + 1) as f32 / d.size as f32,
                    (s.p as f32 / len as f32).min(1.0),
                    if s.busy { 1.0 } else { 0.0 },
                ]);
            }
        }
        TaskView {
            id: d.id.clone(),
            kind: d.kind,
            url: d.url.clone(),
            page: d.page.clone(),
            name: if d.name.is_empty() { d.title.clone() } else { d.name.clone() },
            title: d.title.clone(),
            folder: d.folder.clone(),
            path: d.path.clone(),
            size: d.size,
            done,
            status: d.status,
            error: d.error.clone(),
            category: d.category.clone(),
            thumb: d.thumb.clone(),
            created: d.created,
            finished: d.finished,
            speed,
            eta,
            conns: self.conns.load(Ordering::Relaxed),
            segs,
            stage: d.stage.clone(),
            resumable: d.resumable,
            mode: d.opts.mode.clone(),
            quality: if d.kind == Kind::Media {
                if d.opts.mode == "audio" {
                    format!("{} {}k", d.opts.codec.to_uppercase(), d.opts.bitrate)
                } else if d.opts.height > 0 {
                    format!("{}p {}", d.opts.height, d.opts.container.to_uppercase())
                } else {
                    format!("En iyi {}", d.opts.container.to_uppercase())
                }
            } else {
                String::new()
            },
        }
    }
}

#[derive(Serialize, Clone)]
pub struct TaskView {
    pub id: String,
    pub kind: Kind,
    pub url: String,
    pub page: String,
    pub name: String,
    pub title: String,
    pub folder: String,
    pub path: String,
    pub size: u64,
    pub done: u64,
    pub status: Status,
    pub error: String,
    pub category: String,
    pub thumb: String,
    pub created: i64,
    pub finished: i64,
    pub speed: f64,
    pub eta: f64,
    pub conns: u32,
    pub segs: Vec<[f32; 4]>,
    pub stage: String,
    pub resumable: bool,
    pub mode: String,
    pub quality: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub folder: String,
    pub connections: u32,
    pub concurrent: u32,
    pub speed_limit: u64, // KB/s, 0 sınırsız
    pub categorize: bool,
    pub clipboard: bool,
    pub tray: bool,
    pub sound: bool,
    pub notify: bool,
    pub after_all: String,
    pub auto_resume: bool,
    pub default_height: u32,
    pub default_container: String,
    pub default_codec: String,
    pub default_bitrate: u32,
    pub intercept: bool,
    pub min_intercept_kb: u64,
    /// devralınacak uzantılar, boşlukla ayrılmış; boşsa hepsi
    pub intercept_exts: String,
    /// devralma kapalı siteler (alt alan adları dahil)
    pub intercept_skip: Vec<String>,
    /// bundan büyükse (MB) önce sor, 0 kapalı
    pub intercept_ask_mb: u64,
    pub accent: String,
    pub theme: String,
    pub motion: bool,
    pub motion_mode: String,
    pub density: String,
    pub clipboard_asked: bool,
    pub tray_told: bool,
    pub autostart: bool,
}

pub const DEFAULT_EXTS: &str = "zip rar 7z tar gz tgz bz2 xz zst cab iso img bin exe msi msix appx apk xapk dmg pkg deb rpm appimage jar mp4 mkv avi mov wmv flv webm m4v mpg mpeg 3gp ts mp3 m4a aac flac wav ogg opus wma pdf epub mobi djvu doc docx xls xlsx ppt pptx odt ods odp psd ai torrent";

impl Default for Settings {
    fn default() -> Self {
        let folder = dirs_download().join("Trawl").to_string_lossy().to_string();
        Self {
            folder,
            connections: 8,
            concurrent: 3,
            speed_limit: 0,
            categorize: true,
            clipboard: false,
            tray: true,
            sound: true,
            notify: true,
            after_all: "none".into(),
            auto_resume: true,
            default_height: 1080,
            default_container: "mp4".into(),
            default_codec: "mp3".into(),
            default_bitrate: 320,
            intercept: true,
            min_intercept_kb: 512,
            intercept_exts: DEFAULT_EXTS.into(),
            intercept_skip: vec![],
            intercept_ask_mb: 50,
            accent: "cobalt".into(),
            theme: "system".into(),
            motion_mode: "system".into(),
            density: "default".into(),
            clipboard_asked: false,
            tray_told: false,
            motion: true,
            autostart: false,
        }
    }
}

pub fn dirs_download() -> std::path::PathBuf {
    // kullanıcı İndirilenler'i başka sürücüye taşımış olabilir
    unsafe {
        use windows_sys::Win32::UI::Shell::{FOLDERID_Downloads, SHGetKnownFolderPath};
        let mut p: windows_sys::core::PWSTR = std::ptr::null_mut();
        if SHGetKnownFolderPath(&FOLDERID_Downloads, 0, std::ptr::null_mut(), &mut p) == 0 && !p.is_null() {
            let n = (0..).take_while(|&i| *p.add(i) != 0).count();
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
            windows_sys::Win32::System::Com::CoTaskMemFree(p.cast());
            if !s.is_empty() {
                return std::path::PathBuf::from(s);
            }
        }
    }
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(home).join("Downloads")
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::Est;

    #[test]
    fn eta_warms_up_counts_down_and_stalls() {
        let mut e = Est::default();
        let size = 100u64 << 20; // 100 MB
        let mut done = 0u64;
        let rate = 1u64 << 20; // 1 MB/sn sabit
        let dt = 0.25;
        let mut first_eta = None;
        let mut prev = f64::MAX;
        let mut jumps_up = 0;
        for i in 0..200 {
            let delta = (rate as f64 * dt) as u64;
            done += delta;
            e.update(delta as f64 / dt, dt, done, size);
            if i < 8 {
                assert_eq!(e.disp, -1.0, "ilk 2 sn ısınma: hesaplanıyor olmalı");
            }
            if e.disp >= 0.0 {
                first_eta.get_or_insert(e.disp);
                if e.disp > prev + 0.01 {
                    jumps_up += 1;
                }
                prev = e.disp;
            }
        }
        let first = first_eta.expect("ETA hiç oluşmadı");
        let expect = (size - done) as f64 / rate as f64;
        assert!(jumps_up == 0, "sabit hızda ETA yukarı zıplamamalı ({jumps_up})");
        assert!((e.disp - expect).abs() < 3.0, "son ETA {} beklenen {}", e.disp, expect);
        assert!(first > e.disp);
        // 6 sn veri yoksa -2
        for _ in 0..24 {
            e.update(0.0, dt, done, size);
        }
        assert_eq!(e.disp, -2.0, "takılınca Bekleniyor olmalı");
        // veri tekrar akınca ETA bir süre sonra geri geliyor
        for _ in 0..60 {
            done += (rate as f64 * dt) as u64;
            e.update(rate as f64, dt, done, size);
        }
        assert!(e.disp >= 0.0, "toparlanınca ETA dönmeli, {}", e.disp);
    }
}
