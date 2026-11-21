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
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager as _};
use tokio_util::sync::CancellationToken;

pub const STOPPED: &str = "__durduruldu__";

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
        if wait > 0.0 {
            tokio::time::sleep(Duration::from_secs_f64(wait.min(2.0))).await;
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
}

fn state_path() -> PathBuf {
    data_dir().join("state.json")
}

impl Manager {
    pub fn new(app: AppHandle) -> Arc<Self> {
        let sf: StateFile = std::fs::read(state_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
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
            settings: Mutex::new(sf.settings),
        });
        m.limiter.rate.store(m.settings.lock().speed_limit * 1024, Ordering::Relaxed);
        let mm = m.clone();
        tauri::async_runtime::spawn(async move { mm.run_loop().await });
        m
    }

    pub fn save(&self) {
        let tasks = self
            .tasks
            .lock()
            .iter()
            .map(|t| {
                let mut d = t.d.lock().clone();
                d.done = t.get_done();
                d.was_active = d.status.active() || d.status == Status::Queued;
                d
            })
            .collect();
        let sf = StateFile { settings: self.settings.lock().clone(), tasks };
        if let Ok(b) = serde_json::to_vec(&sf) {
            let tmp = state_path().with_extension("tmp");
            if std::fs::write(&tmp, b).is_ok() {
                let _ = std::fs::rename(tmp, state_path());
            }
        }
    }

    pub fn notice(&self, v: serde_json::Value) {
        let _ = self.app.emit("notice", v);
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            tasks: self.tasks.lock().iter().map(|t| t.view()).collect(),
            speed: *self.total.lock(),
            history: self.history.lock().iter().copied().collect(),
            session: self.session_bytes.load(Ordering::Relaxed),
            eta_total: *self.eta_total.lock(),
        }
    }

    pub fn find(&self, id: &str) -> Option<Arc<Task>> {
        self.tasks.lock().iter().find(|t| t.d.lock().id == id).cloned()
    }

    async fn run_loop(self: Arc<Self>) {
        let mut iv = tokio::time::interval(Duration::from_millis(250));
        let mut prev = Instant::now();
        let mut last_save = Instant::now();
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
                if t.status() == Status::Queued && !t.running.load(Ordering::SeqCst) {
                    self.start(t.clone());
                    slots -= 1;
                }
            }
            let busy = active > 0 || queued;
            if self.was_busy.swap(busy, Ordering::Relaxed) && !busy {
                self.all_done();
            }
            let _ = self.app.emit("tick", self.snapshot());
            if last_save.elapsed() > Duration::from_secs(3) {
                last_save = Instant::now();
                self.save();
            }
        }
    }

    /// Görev çubuğu düğmesinde toplam ilerleme (yalnızca değişince).
    fn taskbar(&self, active: u32, done: u64, size: u64) {
        use tauri::window::{ProgressBarState, ProgressBarStatus};
        let pct = if size > 0 { done * 100 / size } else { 0 };
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
        self.settings.lock().after_all = "none".into();
        self.notice(serde_json::json!({"type": "after_all", "action": act}));
        match act.as_str() {
            "shutdown" => {
                let _ = std::process::Command::new("shutdown").args(["/s", "/t", "60"]).creation_flags(NO_WINDOW).spawn();
            }
            "sleep" => {
                let _ = std::process::Command::new("rundll32.exe")
                    .args(["powrprof.dll,SetSuspendState", "0,1,0"])
                    .creation_flags(NO_WINDOW)
                    .spawn();
            }
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
            let id = t.id();
            let paused = m.pausing.lock().remove(&id);
            match r {
                Ok(()) => {
                    let (name, path) = {
                        let mut d = t.d.lock();
                        d.status = Status::Done;
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
        let cat = if cat == "diger" { category_of(&name_from_url(&url)) } else { cat };
        self.push(TaskData {
            id: uuid::Uuid::new_v4().simple().to_string()[..12].to_string(),
            kind: Kind::Http,
            url,
            page,
            name,
            folder: if folder.is_empty() { self.folder_for(&cat) } else { folder },
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
        self.push(TaskData {
            id: uuid::Uuid::new_v4().simple().to_string()[..12].to_string(),
            kind: Kind::Media,
            url,
            title,
            thumb,
            folder: if folder.is_empty() { self.folder_for(cat) } else { folder },
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
        if matches!(d.status, Status::Paused | Status::Error) && !t.running.load(Ordering::SeqCst) {
            if d.status == Status::Error && d.kind == Kind::Http && !d.resumable {
                d.segs.clear();
                t.set_done(0);
            }
            d.status = Status::Queued;
            d.error.clear();
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
        d.segs.clear();
        d.size = 0;
        d.status = Status::Queued;
        d.error.clear();
        t.set_done(0);
    }

    pub async fn remove(&self, id: &str, delete_file: bool) {
        let Some(t) = self.find(id) else { return };
        t.removing.store(true, Ordering::SeqCst);
        t.token().cancel();
        for _ in 0..60 {
            if !t.running.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        self.tasks.lock().retain(|x| !Arc::ptr_eq(x, &t));
        let d = t.d.lock().clone();
        if d.status != Status::Done {
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
        let (kind, size, headers) = {
            let d = t.d.lock();
            (d.kind, d.size, d.headers.clone())
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
            final_url = p.url;
        }
        let mut d = t.d.lock();
        d.url = final_url;
        d.status = Status::Queued;
        d.error.clear();
        Ok(())
    }

    pub fn move_task(&self, id: &str, to: usize) {
        let mut v = self.tasks.lock();
        if let Some(i) = v.iter().position(|t| t.d.lock().id == id) {
            let t = v.remove(i);
            let to = to.min(v.len());
            v.insert(to, t);
        }
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
        self.limiter.rate.store(s.speed_limit * 1024, Ordering::Relaxed);
        *self.settings.lock() = s;
        self.save();
    }

    /// aynı url ya da aynı adla bitmiş ve dosyası duran kayıt; (id, durum, diskte mi)
    pub fn find_dup(&self, url: &str, name: &str) -> Option<(String, Status, bool)> {
        let name = safe_name(name);
        self.tasks.lock().iter().find_map(|t| {
            let d = t.d.lock();
            let exists = !d.path.is_empty() && std::path::Path::new(&d.path).exists();
            let same = d.url == url || (!name.is_empty() && d.status == Status::Done && d.name.eq_ignore_ascii_case(&name) && exists);
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
