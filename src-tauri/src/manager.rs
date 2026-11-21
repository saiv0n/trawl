use crate::model::*;
use crate::util::*;
use crate::{http, media};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use chrono::Datelike;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager as _};
use tokio_util::sync::CancellationToken;

pub const STOPPED: &str = "__durduruldu__";
const NET_RETRIES: u32 = 6;

fn net_error(e: &anyhow::Error) -> bool {
    let s = format!("{e:#}");
    e.chain().any(|c| c.downcast_ref::<reqwest::Error>().is_some_and(|r| r.is_connect() || r.is_timeout() || r.is_request() || r.is_body()))
        || s.contains("zaman aşımı")
        || s.contains("Sunucu yanıt vermiyor")
}

fn suspend(hibernate: bool) {
    std::thread::spawn(move || unsafe {
        windows_sys::Win32::System::Power::SetSuspendState(hibernate.into(), false.into(), false.into());
    });
}

pub struct Limiter {
    pub rate: AtomicU64,
    st: Mutex<(f64, Instant)>,
}

impl Limiter {
    pub fn new() -> Self {
        Self { rate: AtomicU64::new(0), st: Mutex::new((0.0, Instant::now())) }
    }
    pub async fn take(&self, n: u64) {
        let r = self.rate.load(Ordering::Relaxed) as f64;
        if r <= 0.0 {
            return;
        }
        let wait = {
            let mut g = self.st.lock();
            let now = Instant::now();
            g.0 = (g.0 + now.duration_since(g.1).as_secs_f64() * r).min(r);
            g.1 = now;
            g.0 -= n as f64;
            if g.0 < 0.0 { -g.0 / r } else { 0.0 }
        };
        // borcun tamamı beklenmeli, üst sınır koyunca çok bağlantıda sınır delinir.
        // sınır kalkarsa ya da artarsa beklemeyi kısalt
        let end = Instant::now() + Duration::from_secs_f64(wait.min(60.0));
        while Instant::now() < end {
            let r2 = self.rate.load(Ordering::Relaxed) as f64;
            if r2 <= 0.0 || r2 > r * 1.5 {
                return;
            }
            tokio::time::sleep((end - Instant::now()).min(Duration::from_millis(250))).await;
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct StateFile {
    settings: Settings,
    tasks: Vec<TaskData>,
}

#[derive(Serialize, Clone)]
pub struct Snapshot {
    pub tasks: Vec<TaskView>,
    pub speed: f64,
    pub history: Vec<f64>,
    pub session: u64,
    pub eta_total: f64,
    /// doluysa tasks yalnızca değişenler, sıra buradan
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<String>>,
}

pub struct Manager {
    pub tasks: Mutex<Vec<Arc<Task>>>,
    pub settings: Mutex<Settings>,
    pub limiter: Arc<Limiter>,
    pub app: AppHandle,
    history: Mutex<VecDeque<f64>>,
    total: Mutex<f64>,
    eta_total: Mutex<f64>,
    taskbar_last: Mutex<(u32, u64)>,
    session_bytes: AtomicU64,
    session_done: AtomicU32,
    pausing: Mutex<std::collections::HashSet<String>>,
    was_busy: AtomicBool,
    /// tazeleme işleri, eklenti /jobs'tan alıyor
    pub refresh: Mutex<Vec<crate::refresh::Job>>,
    /// eklentinin son görüldüğü an (sn), tazeleme sadece eklenti canlıyken
    pub ext_seen: AtomicI64,
    /// (sürüm, tarayıcı)
    pub ext_info: Mutex<(String, String)>,
    /// köprü portu açılamadıysa nedeni
    pub bridge_err: Mutex<String>,
    last_views: Mutex<std::collections::HashMap<String, String>>,
    last_saved: Mutex<Vec<u8>>,
    /// zamanlayıcı aralığında mıydık, None henüz bakılmadı
    sched_in: Mutex<Option<bool>>,
    sched_ids: Mutex<std::collections::HashSet<String>>,
    ui_up: AtomicBool,
    pending: Mutex<Vec<serde_json::Value>>,
}

/// o anki genel sınır (KB/s), aralıktaysa ikinci sınır
fn rate_now(s: &Settings) -> u64 {
    let hm = chrono::Local::now().format("%H:%M").to_string();
    if s.alt_limit && in_window(&hm, &s.alt_start, &s.alt_stop) { s.alt_speed } else { s.speed_limit }
}

/// "SS:DD" karşılaştırması, gece yarısını aşan aralık da olur (23:00-06:00)
fn in_window(now: &str, start: &str, stop: &str) -> bool {
    if start == stop {
        return false;
    }
    if start < stop { start <= now && now < stop } else { now >= start || now < stop }
}

fn state_path() -> PathBuf {
    data_dir().join("state.json")
}

fn read_state(p: &std::path::Path) -> Option<StateFile> {
    let mut sf: StateFile = serde_json::from_slice(&std::fs::read(p).ok()?).ok()?;
    for t in sf.tasks.iter_mut() {
        unseal_headers(&mut t.headers);
    }
    for l in sf.settings.logins.iter_mut() {
        l.pass = unseal_str(&l.pass);
    }
    Some(sf)
}

// bozuk dosyanın üstüne boş durum yazılmasın: kenara al, yedekten aç
fn load_state() -> StateFile {
    let p = state_path();
    let bak = p.with_extension("bak");
    if let Some(sf) = read_state(&p) {
        let _ = std::fs::copy(&p, &bak);
        return sf;
    }
    if p.exists() {
        let _ = std::fs::rename(&p, p.with_extension(format!("bozuk-{}.json", now())));
    }
    read_state(&bak).unwrap_or_default()
}

impl Manager {
    pub fn new(app: AppHandle) -> Arc<Self> {
        let sf = load_state();
        let auto = sf.settings.auto_resume;
        let tasks = sf
            .tasks
            .into_iter()
            .map(|mut d| {
                if d.status.active() || d.status == Status::Queued {
                    d.status = if auto && d.was_active { Status::Queued } else { Status::Paused };
                }
                Arc::new(Task::new(d))
            })
            .collect();
        let m = Arc::new(Self {
            tasks: Mutex::new(tasks),
            limiter: Arc::new(Limiter::new()),
            app,
            history: Mutex::new(VecDeque::from(vec![0.0; 120])),
            total: Mutex::new(0.0),
            eta_total: Mutex::new(-1.0),
            taskbar_last: Mutex::new((u32::MAX, u64::MAX)),
            session_bytes: AtomicU64::new(0),
            session_done: AtomicU32::new(0),
            pausing: Mutex::new(Default::default()),
            was_busy: AtomicBool::new(false),
            refresh: Mutex::new(vec![]),
            ext_seen: AtomicI64::new(0),
            ext_info: Mutex::new(Default::default()),
            bridge_err: Mutex::new(String::new()),
            last_views: Mutex::new(Default::default()),
            last_saved: Mutex::new(vec![]),
            sched_in: Mutex::new(None),
            sched_ids: Mutex::new(Default::default()),
            ui_up: AtomicBool::new(false),
            pending: Mutex::new(vec![]),
            settings: Mutex::new(sf.settings),
        });
        m.limiter.rate.store(rate_now(&m.settings.lock()) * 1024, Ordering::Relaxed);
        set_proxy(&m.settings.lock().proxy);
        http::HOST_CONNS.store(m.settings.lock().host_conns, Ordering::Relaxed);
        set_logins(&m.settings.lock().logins);
        *media::COOKIES_FROM.write() = m.settings.lock().cookies_from.clone();
        let mm = m.clone();
        tauri::async_runtime::spawn(async move { mm.run_loop().await });
        m
    }

    pub fn save(&self) {
        let list = self.tasks.lock().clone();
        let tasks = list
            .iter()
            .map(|t| {
                let mut d = t.d.lock().clone();
                d.done = t.get_done();
                seal_headers(&mut d.headers);
                d.was_active = d.status.active() || d.status == Status::Queued;
                d
            })
            .collect();
        // kayıttaki baytlar diske inmeden yazılırsa elektrik kesintisinde dosya sessizce bozulur
        for t in &list {
            let f = t.file.lock().clone();
            if let Some(f) = f {
                let _ = f.sync_data();
            }
        }
        let mut settings = self.settings.lock().clone();
        for l in settings.logins.iter_mut() {
            l.pass = seal_str(&l.pass);
        }
        let sf = StateFile { settings, tasks };
        if let Ok(b) = serde_json::to_vec(&sf) {
            // değişmediyse diske dokunma
            let mut last = self.last_saved.lock();
            if *last == b {
                return;
            }
            let tmp = state_path().with_extension("tmp");
            let ok = std::fs::File::create(&tmp).and_then(|mut f| {
                use std::io::Write;
                f.write_all(&b)?;
                f.sync_all()
            });
            if ok.is_ok() && std::fs::rename(tmp, state_path()).is_ok() {
                *last = b;
            }
        }
    }

    /// arayüz henüz dinlemiyorsa (eklenti uygulamayı yeni açtıysa) bekletilir, yoksa kaybolurdu
    pub fn notice(&self, v: serde_json::Value) {
        if !self.ui_up.load(Ordering::SeqCst) {
            self.pending.lock().push(v);
            return;
        }
        let _ = self.app.emit("notice", v);
    }

    pub fn ui_ready(&self) {
        let list = {
            let mut p = self.pending.lock();
            self.ui_up.store(true, Ordering::SeqCst);
            std::mem::take(&mut *p)
        };
        for v in list {
            let _ = self.app.emit("notice", v);
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            tasks: self.tasks.lock().iter().map(|t| t.view()).collect(),
            speed: *self.total.lock(),
            history: self.history.lock().iter().copied().collect(),
            session: self.session_bytes.load(Ordering::Relaxed),
            eta_total: *self.eta_total.lock(),
            ids: None,
        }
    }

    /// her tıkta bütün listeyi göndermek yerine değişen satırlar, 2 sn'de bir tamamı
    fn tick_snapshot(&self, full: bool) -> Snapshot {
        let mut s = self.snapshot();
        let mut last = self.last_views.lock();
        let ids: Vec<String> = s.tasks.iter().map(|t| t.id.clone()).collect();
        let mut fresh = std::collections::HashMap::new();
        let mut changed = vec![];
        for v in s.tasks.drain(..) {
            let j = serde_json::to_string(&v).unwrap_or_default();
            if full || last.get(&v.id) != Some(&j) {
                changed.push(v.clone());
            }
            fresh.insert(v.id, j);
        }
        *last = fresh;
        s.tasks = changed;
        if !full {
            s.ids = Some(ids);
        }
        s
    }

    pub fn find(&self, id: &str) -> Option<Arc<Task>> {
        self.tasks.lock().iter().find(|t| t.d.lock().id == id).cloned()
    }

    async fn run_loop(self: Arc<Self>) {
        let mut iv = tokio::time::interval(Duration::from_millis(250));
        let mut prev = Instant::now();
        let mut last_save = Instant::now();
        let mut ticks = 0u64;
        let mut minute = String::new();
        let mut saving: Option<tokio::task::JoinHandle<()>> = None;
        loop {
            iv.tick().await;
            let dt = prev.elapsed().as_secs_f64().max(0.001);
            prev = Instant::now();
            let tasks = self.tasks.lock().clone();
            let mut total = 0.0;
            let mut total8 = 0.0;
            let mut left_all = 0u64;
            let (mut sum_done, mut sum_size) = (0u64, 0u64);
            let mut active = 0u32;
            let mut queued = false;
            for t in &tasks {
                let st = t.status();
                let done = t.get_done();
                let size = t.d.lock().size;
                let last = t.last.swap(done, Ordering::Relaxed);
                let mut sp = t.speed.lock();
                let mut est = t.est.lock();
                // tazeleme bekleyen görevin çalışan işi yok, yer tutmasın
                if st.active() && t.running.load(Ordering::SeqCst) {
                    active += 1;
                    let delta = done.saturating_sub(last);
                    self.session_bytes.fetch_add(delta, Ordering::Relaxed);
                    est.update(delta as f64 / dt, dt, done, size);
                    *sp = est.v2.max(0.0);
                    total += *sp;
                    total8 += est.v8.max(0.0);
                    if size > done {
                        left_all += size - done;
                    }
                    if size > 0 {
                        sum_done += done.min(size);
                        sum_size += size;
                    }
                } else {
                    *sp = 0.0;
                    *est = Default::default();
                }
                if st == Status::Queued {
                    queued = true;
                }
            }
            *self.total.lock() = total;
            *self.eta_total.lock() = if total8 > 1.0 && left_all > 0 { left_all as f64 / total8 } else { -1.0 };
            self.taskbar(active, sum_done, sum_size);
            {
                let mut h = self.history.lock();
                h.pop_front();
                h.push_back(total);
            }
            let max = self.settings.lock().concurrent.max(1);
            let mut slots = max.saturating_sub(active);
            for t in &tasks {
                if slots == 0 {
                    break;
                }
                if t.status() == Status::Queued && !t.running.load(Ordering::SeqCst) && t.d.lock().retry_at <= now() {
                    self.start(t.clone());
                    slots -= 1;
                }
            }
            let busy = active > 0 || queued;
            if self.was_busy.swap(busy, Ordering::Relaxed) && !busy {
                self.all_done();
            }
            ticks += 1;
            let hm = chrono::Local::now().format("%H:%M").to_string();
            if hm != minute {
                minute = hm;
                self.schedule_tick(&minute);
                self.limiter.rate.store(rate_now(&self.settings.lock()) * 1024, Ordering::Relaxed);
            }
            let _ = self.app.emit("tick", self.tick_snapshot(ticks.is_multiple_of(8)));
            // sync_data yavaş diskte uzun sürebilir, tik beklemesin
            if last_save.elapsed() > Duration::from_secs(3) && saving.as_ref().is_none_or(|h| h.is_finished()) {
                last_save = Instant::now();
                let m = self.clone();
                saving = Some(tokio::task::spawn_blocking(move || m.save()));
            }
        }
    }

    /// zamanlayıcı: aralığa girince sıradakileri başlat, çıkınca onları duraklat.
    /// tam dakikaya bakmıyoruz, uykudan sonra kaçan geçişi de yakalasın
    fn schedule_tick(&self, hm: &str) {
        let (on, start, stop, days) = {
            let s = self.settings.lock();
            (s.schedule, s.schedule_start.clone(), s.schedule_stop.clone(), s.schedule_days.clone())
        };
        let mut prev = self.sched_in.lock();
        if !on {
            *prev = None;
            self.sched_ids.lock().clear();
            return;
        }
        let today = chrono::Local::now().weekday().num_days_from_monday() as u8;
        // gece yarısını aşan aralıkta sabah kısmı dünün aralığı
        let day = if start > stop && hm < stop.as_str() { (today + 6) % 7 } else { today };
        let inside = in_window(hm, &start, &stop) && days.contains(&day);
        let was = prev.replace(inside);
        // ilk bakışta bir şey yapma, açılışta kullanıcının duraklattıkları başlamasın
        if was.is_none() || was == Some(inside) {
            return;
        }
        if inside {
            let ids: Vec<String> = self.tasks.lock().iter().filter(|t| t.status() == Status::Paused).map(|t| t.id()).collect();
            for i in &ids {
                self.resume(i);
            }
            *self.sched_ids.lock() = ids.into_iter().collect();
        } else {
            // elle başlatılanlara dokunma, sadece bizim başlattıklarımız ve sırada bekleyenler
            let ids = std::mem::take(&mut *self.sched_ids.lock());
            let list: Vec<String> = self.tasks.lock().iter().filter(|t| ids.contains(&t.id()) || t.status() == Status::Queued).map(|t| t.id()).collect();
            for i in list {
                self.pause(&i);
            }
        }
        self.notice(serde_json::json!({"type": "schedule", "start": inside}));
    }

    /// Görev çubuğu düğmesinde toplam ilerleme (yalnızca değişince).
    fn taskbar(&self, active: u32, done: u64, size: u64) {
        use tauri::window::{ProgressBarState, ProgressBarStatus};
        let pct = (done * 100).checked_div(size).unwrap_or(0);
        let key = (active, if active > 0 && size == 0 { 1000 } else { pct });
        {
            let mut last = self.taskbar_last.lock();
            if *last == key {
                return;
            }
            *last = key;
        }
        let Some(w) = self.app.get_webview_window("main") else { return };
        let state = if active == 0 {
            ProgressBarState { status: Some(ProgressBarStatus::None), progress: None }
        } else if size == 0 {
            ProgressBarState { status: Some(ProgressBarStatus::Indeterminate), progress: None }
        } else {
            ProgressBarState { status: Some(ProgressBarStatus::Normal), progress: Some(pct) }
        };
        let _ = w.set_progress_bar(state);
    }

    fn all_done(&self) {
        let act = self.settings.lock().after_all.clone();
        if act == "none" || self.session_done.load(Ordering::Relaxed) == 0 {
            return;
        }
        // arayüz eski değeri geri yazmasın
        let s = {
            let mut g = self.settings.lock();
            g.after_all = "none".into();
            g.clone()
        };
        let _ = self.app.emit("settings-changed", &s);
        self.notice(serde_json::json!({"type": "after_all", "action": act}));
        match act.as_str() {
            "shutdown" => {
                let _ = std::process::Command::new("shutdown").args(["/s", "/t", "60"]).creation_flags(NO_WINDOW).spawn();
            }
            // rundll32 ile çağırınca argümanlar yanlış geçiyor, hazırda bekletme açıksa uyutmak yerine onu yapıyordu
            "sleep" => suspend(false),
            "hibernate" => suspend(true),
            "quit" => {
                self.save();
                self.app.exit(0);
            }
            _ => {}
        }
    }

    fn start(self: &Arc<Self>, t: Arc<Task>) {
        *t.cancel.lock() = CancellationToken::new();
        t.running.store(true, Ordering::SeqCst);
        {
            let mut d = t.d.lock();
            d.status = if d.kind == Kind::Http { Status::Probing } else { Status::Downloading };
            d.error.clear();
            d.stage.clear();
        }
        t.last.store(t.get_done(), Ordering::Relaxed);
        let m = self.clone();
        tauri::async_runtime::spawn(async move {
            let kind = t.d.lock().kind;
            let r = match kind {
                Kind::Http => http::run(t.clone(), m.clone()).await,
                Kind::Media => media::run(t.clone(), m.clone()).await,
            };
            t.conns.store(0, Ordering::Relaxed);
            // sağlama ya da tarama tutmazsa normal bir hata gibi gösterilsin
            let r = match r {
                Ok(()) => crate::post::run(&m, &t).await.map_err(|x| anyhow::anyhow!(x)),
                e => e,
            };
            let id = t.id();
            let paused = m.pausing.lock().remove(&id);
            match r {
                Ok(()) => {
                    let (name, path) = {
                        let mut d = t.d.lock();
                        d.status = Status::Done;
                        d.net_retries = 0;
                        d.retry_at = 0;
                        d.finished = now();
                        d.refresh_tries = 0;
                        d.stage.clear();
                        d.segs.clear();
                        if d.size == 0 {
                            d.size = t.get_done();
                        }
                        t.set_done(d.size);
                        (if d.name.is_empty() { d.title.clone() } else { d.name.clone() }, d.path.clone())
                    };
                    m.session_done.fetch_add(1, Ordering::Relaxed);
                    m.notice(serde_json::json!({"type": "done", "id": id, "name": name, "path": path}));
                    m.os_notify("İndirme tamamlandı", &name);
                    m.save();
                }
                Err(e) => {
                    let removing = t.removing.load(Ordering::SeqCst);
                    if removing {
                        // remove() temizliyor
                    } else if paused || e.to_string() == STOPPED {
                        let mut d = t.d.lock();
                        if d.status != Status::Queued {
                            d.status = Status::Paused;
                        }
                        if d.kind == Kind::Media {
                            d.stage.clear();
                        }
                    } else if crate::refresh::try_start(&m, &t, &format!("{e:#}")) {
                        // eklenti sayfayı açıp yeni bağlantıyı getirecek
                    } else if net_error(&e) && t.d.lock().net_retries < NET_RETRIES {
                        // ağ gitmiş, biraz sonra kendiliğinden yeniden dene
                        let mut d = t.d.lock();
                        let wait = 15i64 << d.net_retries.min(5);
                        d.net_retries += 1;
                        d.retry_at = now() + wait;
                        d.status = Status::Queued;
                        d.stage = "Ağ bekleniyor".into();
                    } else if format!("{e:#}").contains("parçalı indirmeyi reddetti") && t.d.lock().conns_max != 1 {
                        // sunucu ortadan parça vermiyor, kullanıcıya bırakmadan tek bağlantıyla baştan al
                        let mut d = t.d.lock();
                        if !d.path.is_empty() {
                            let _ = std::fs::remove_file(format!("{}.trawl", d.path));
                            d.path.clear();
                        }
                        d.conns_max = 1;
                        d.segs.clear();
                        d.size = 0;
                        d.validator.clear();
                        d.status = Status::Queued;
                        t.set_done(0);
                    } else {
                        let msg = format!("{e:#}");
                        let (msg, tools) = match msg.strip_prefix("TOOLS:") {
                            Some(x) => (x.to_string(), true),
                            None => (clean_err(&msg), false),
                        };
                        let name = {
                            let mut d = t.d.lock();
                            d.status = Status::Error;
                            d.error = msg.clone();
                            d.name.clone()
                        };
                        m.os_notify("İndirilemedi", &format!("{name}: {msg}"));
                        m.notice(serde_json::json!({"type": "error", "id": id, "name": name, "error": msg, "tools": tools}));
                    }
                }
            }
            *t.speed.lock() = 0.0;
            t.running.store(false, Ordering::SeqCst);
        });
    }

    pub fn os_notify(&self, title: &str, body: &str) {
        if !self.settings.lock().notify {
            return;
        }
        let visible = self
            .app
            .get_webview_window("main")
            .map(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false))
            .unwrap_or(false);
        if visible {
            return;
        }
        use tauri_plugin_notification::NotificationExt;
        let _ = self.app.notification().builder().title(title).body(body).show();
    }

    /// kullanıcının kuralı tutarsa onun klasörü, yoksa türün klasörü
    pub fn folder_by_rule(&self, name: &str, url: &str, cat: &str) -> String {
        let hit = self.settings.lock().rules.iter().find(|r| r.hits(name, url)).map(|r| r.folder.clone());
        hit.unwrap_or_else(|| self.folder_for(cat))
    }

    pub fn folder_for(&self, cat: &str) -> String {
        let s = self.settings.lock();
        if s.categorize {
            PathBuf::from(&s.folder).join(cat_dir(cat)).to_string_lossy().to_string()
        } else {
            s.folder.clone()
        }
    }

    fn push(&self, d: TaskData) -> String {
        let id = d.id.clone();
        let name = d.name.clone();
        self.tasks.lock().push(Arc::new(Task::new(d)));
        self.notice(serde_json::json!({"type": "added", "id": id, "name": name}));
        self.save();
        id
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_http(&self, url: String, name: String, headers: BTreeMap<String, String>, folder: String, conns: u32, start: bool, page: String) -> String {
        let name = if name.is_empty() { String::new() } else { safe_name(&name) };
        let cat = category_of(if name.is_empty() { &url } else { &name });
        let fallback = name_from_url(&url);
        let cat = if cat == "diger" { category_of(&fallback) } else { cat };
        let folder = if folder.is_empty() { self.folder_by_rule(if name.is_empty() { &fallback } else { &name }, &url, &cat) } else { folder };
        self.push(TaskData {
            id: uuid::Uuid::new_v4().simple().to_string()[..12].to_string(),
            kind: Kind::Http,
            url,
            page,
            name,
            folder,
            category: cat,
            headers,
            conns_max: conns,
            status: if start { Status::Queued } else { Status::Paused },
            ..Default::default()
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_media(&self, url: String, opts: MediaOpts, title: String, thumb: String, folder: String, headers: BTreeMap<String, String>, start: bool) -> String {
        let cat = if opts.mode == "audio" { "muzik" } else { "video" };
        let folder = if folder.is_empty() {
            let ext = if opts.mode == "audio" { &opts.codec } else { &opts.container };
            self.folder_by_rule(&format!("x.{ext}"), &url, cat)
        } else {
            folder
        };
        self.push(TaskData {
            id: uuid::Uuid::new_v4().simple().to_string()[..12].to_string(),
            kind: Kind::Media,
            url,
            title,
            thumb,
            folder,
            category: cat.into(),
            headers,
            opts,
            status: if start { Status::Queued } else { Status::Paused },
            ..Default::default()
        })
    }

    pub fn pause(&self, id: &str) {
        let Some(t) = self.find(id) else { return };
        let st = t.status();
        if st == Status::Queued || (st.active() && !t.running.load(Ordering::SeqCst)) {
            // sırada ya da tazeleme bekliyor, çalışan iş yok
            self.refresh.lock().retain(|j| j.id != id);
            let mut d = t.d.lock();
            d.status = Status::Paused;
            d.stage.clear();
        } else if st.active() && st != Status::Processing {
            self.pausing.lock().insert(id.to_string());
            t.token().cancel();
        }
    }

    pub fn resume(&self, id: &str) {
        let Some(t) = self.find(id) else { return };
        let mut d = t.d.lock();
        // duraklatma henüz bitmediyse iş bitince sıraya girsin
        if t.running.load(Ordering::SeqCst) && self.pausing.lock().contains(id) {
            d.status = Status::Queued;
            return;
        }
        if matches!(d.status, Status::Paused | Status::Error) && !t.running.load(Ordering::SeqCst) {
            if d.status == Status::Error && d.kind == Kind::Http && !d.resumable {
                d.segs.clear();
                t.set_done(0);
            }
            d.status = Status::Queued;
            d.error.clear();
            d.net_retries = 0;
            d.retry_at = 0;
            d.stage.clear();
        }
    }

    pub fn start_now(self: &Arc<Self>, id: &str) {
        let Some(t) = self.find(id) else { return };
        if !t.running.load(Ordering::SeqCst) && matches!(t.status(), Status::Paused | Status::Queued | Status::Error) {
            self.start(t);
        }
    }

    pub fn restart(&self, id: &str) {
        let Some(t) = self.find(id) else { return };
        if t.running.load(Ordering::SeqCst) {
            return;
        }
        let mut d = t.d.lock();
        if d.kind == Kind::Http && !d.path.is_empty() {
            let _ = std::fs::remove_file(format!("{}.trawl", d.path));
            d.path.clear();
        }
        if d.error.contains("parçalı") {
            d.conns_max = 1;
        }
        d.segs.clear();
        d.size = 0;
        d.validator.clear();
        d.net_retries = 0;
        d.retry_at = 0;
        d.status = Status::Queued;
        d.error.clear();
        t.set_done(0);
    }

    pub async fn remove(&self, id: &str, delete_file: bool) {
        let Some(t) = self.find(id) else { return };
        t.removing.store(true, Ordering::SeqCst);
        t.token().cancel();
        for _ in 0..300 {
            if !t.running.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        // hâlâ yazıyorsa parça dosyasını silmek bozuk dosya bırakır, olduğu gibi kalsın
        let stopped = !t.running.load(Ordering::SeqCst);
        self.tasks.lock().retain(|x| !Arc::ptr_eq(x, &t));
        let d = t.d.lock().clone();
        if d.status != Status::Done && stopped {
            if !d.path.is_empty() {
                let _ = std::fs::remove_file(format!("{}.trawl", d.path));
            }
            if d.kind == Kind::Media {
                media::cleanup(&d.folder, &d.title);
            }
        }
        if delete_file && !d.path.is_empty() && std::path::Path::new(&d.path).exists() {
            // kalıcı silme yok, çöp kutusuna
            if trash::delete(&d.path).is_err() {
                let _ = std::fs::remove_file(&d.path);
            }
        }
        self.save();
    }

    /// süresi dolan bağlantıyı değiştirir, inen baytlar kalır
    pub async fn update_url(&self, id: &str, url: String) -> Result<(), String> {
        let t = self.find(id).ok_or("İndirme bulunamadı")?;
        if t.running.load(Ordering::SeqCst) {
            return Err("Önce duraklatın.".into());
        }
        let (kind, size, headers, validator) = {
            let d = t.d.lock();
            (d.kind, d.size, d.headers.clone(), d.validator.clone())
        };
        let mut final_url = url.clone();
        if kind == Kind::Http {
            let p = http::probe(&url, &headers).await.map_err(|e| clean_err(&format!("{e:#}")))?;
            if size > 0 && p.size > 0 && p.size != size {
                return Err(format!(
                    "Bu bağlantı farklı bir dosyaya gidiyor (boyut {} değil {}). Doğru sayfadan kopyaladığınızdan emin olun.",
                    p.size, size
                ));
            }
            if !validator.is_empty() && !p.validator.is_empty() && p.validator != validator {
                return Err("Bu bağlantıdaki dosya indirmeye başladığınızdan farklı (sunucuda değişmiş). Baştan indirmeniz gerekiyor.".into());
            }
            final_url = p.url;
        }
        let mut d = t.d.lock();
        d.url = final_url;
        d.status = Status::Queued;
        d.error.clear();
        Ok(())
    }

    /// KB/s, 0 sınırsız. medya işinde yt-dlp yeniden başlayınca geçerli olur
    pub fn set_task_limit(&self, id: &str, kb: u64) {
        let Some(t) = self.find(id) else { return };
        t.d.lock().speed_limit = kb;
        t.lim.rate.store(kb * 1024, Ordering::Relaxed);
        self.save();
    }

    pub fn move_task(&self, id: &str, to: usize) {
        let mut v = self.tasks.lock();
        if let Some(i) = v.iter().position(|t| t.d.lock().id == id) {
            let t = v.remove(i);
            let to = to.min(v.len());
            v.insert(to, t);
        }
        drop(v);
        self.save();
    }

    pub fn pause_all(&self) {
        let ids: Vec<String> = self.tasks.lock().iter().map(|t| t.id()).collect();
        for i in ids {
            self.pause(&i);
        }
    }

    pub fn resume_all(&self) {
        let ids: Vec<String> = self.tasks.lock().iter().filter(|t| t.status() == Status::Paused).map(|t| t.id()).collect();
        for i in ids {
            self.resume(&i);
        }
    }

    pub fn clear_done(&self) {
        self.tasks.lock().retain(|t| t.status() != Status::Done);
        self.save();
    }

    pub fn set_settings(&self, s: Settings) {
        set_proxy(&s.proxy);
        http::HOST_CONNS.store(s.host_conns, Ordering::Relaxed);
        set_logins(&s.logins);
        *media::COOKIES_FROM.write() = s.cookies_from.clone();
        self.limiter.rate.store(rate_now(&s) * 1024, Ordering::Relaxed);
        *self.settings.lock() = s;
        self.save();
    }

    /// aynı url ya da aynı adla bitmiş ve dosyası duran kayıt; (id, durum, diskte mi)
    pub fn find_dup(&self, url: &str, name: &str) -> Option<(String, Status, bool)> {
        let name = safe_name(name);
        self.tasks.lock().iter().find_map(|t| {
            let d = t.d.lock();
            let exists = !d.path.is_empty() && std::path::Path::new(&d.path).exists();
            // sadece adı tutan başka sitenin setup.exe'si yeni indirmeyi engellemesin
            let src = if d.src.is_empty() { &d.url } else { &d.src };
            let same_site = host_of(src) == host_of(url) || host_of(&d.page) == host_of(url);
            let same = d.url == url || d.src == url || (!name.is_empty() && d.status == Status::Done && d.name.eq_ignore_ascii_case(&name) && exists && same_site);
            same.then(|| (d.id.clone(), d.status, exists))
        })
    }

    pub fn path_of(&self, id: &str) -> Option<(String, String)> {
        self.find(id).map(|t| {
            let d = t.d.lock();
            (d.path.clone(), d.folder.clone())
        })
    }

    pub fn shutdown(&self) {
        for t in self.tasks.lock().iter() {
            if t.status().active() {
                t.token().cancel();
            }
        }
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::in_window;

    /// eski sürümün kaydı yeni alanlar olmadan açılmalı, varsayılanlar gelmeli
    #[test]
    fn old_state_gets_new_defaults() {
        let old = r#"{"settings":{"folder":"D:/x","connections":8,"speed_limit":0,"schedule":false},"tasks":[{"id":"a","kind":"http","url":"https://x.test/a.zip","status":"paused","size":10,"done":5}]}"#;
        let sf: super::StateFile = serde_json::from_str(old).unwrap();
        let s = &sf.settings;
        assert_eq!((s.host_conns, s.auto_hash, s.lang.as_str(), s.schedule_days.len()), (16, true, "tr", 7));
        assert!(s.rules.is_empty() && s.logins.is_empty() && !s.scan && !s.extract);
        assert_eq!((sf.tasks[0].speed_limit, sf.tasks[0].net_retries, sf.tasks[0].retry_at), (0, 0, 0));
    }

    #[test]
    fn schedule_window() {
        assert!(in_window("02:30", "02:00", "06:00"));
        assert!(!in_window("06:00", "02:00", "06:00"));
        assert!(in_window("23:30", "23:00", "06:00"));
        assert!(in_window("01:00", "23:00", "06:00"));
        assert!(!in_window("12:00", "23:00", "06:00"));
        assert!(!in_window("12:00", "12:00", "12:00"));
    }
}
