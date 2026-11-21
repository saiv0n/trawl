mod bridge;
mod http;
mod manager;
mod media;
mod model;
mod native;
mod post;
mod refresh;
mod scrape;
mod stream;
mod tools;
mod util;
#[cfg(test)]
mod e2e_tests;
#[cfg(test)]
mod e2e_more;

use manager::{Manager, Snapshot};
use model::{MediaOpts, Settings};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::os::windows::process::CommandExt;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager as _, State, WindowEvent};
use util::*;

type M<'a> = State<'a, Arc<Manager>>;
type R<T> = Result<T, String>;

fn e<E: std::fmt::Display>(x: E) -> String {
    clean_err(&x.to_string())
}

#[derive(Deserialize, Default, Clone)]
#[serde(default)]
struct EntryIn {
    url: String,
    title: String,
    thumb: String,
}

#[derive(Deserialize, Default, Clone)]
#[serde(default)]
struct AddIn {
    kind: String,
    url: String,
    name: String,
    title: String,
    thumb: String,
    folder: String,
    headers: BTreeMap<String, String>,
    opts: MediaOpts,
    conns: u32,
    start: Option<bool>,
    page: String,
    entries: Vec<EntryIn>,
}

fn add_one(m: &Manager, p: AddIn) -> Vec<String> {
    let start = p.start.unwrap_or(true);
    if p.kind == "http" {
        return vec![m.add_http(p.url, p.name, p.headers, p.folder, p.conns, start, p.page)];
    }
    if !p.entries.is_empty() {
        return p
            .entries
            .into_iter()
            .map(|en| m.add_media(en.url, p.opts.clone(), en.title, en.thumb, p.folder.clone(), p.headers.clone(), start))
            .collect();
    }
    vec![m.add_media(p.url, p.opts, p.title, p.thumb, p.folder, p.headers, start)]
}

#[tauri::command]
fn snapshot(m: M) -> Snapshot {
    m.snapshot()
}

#[tauri::command]
fn get_settings(m: M) -> Settings {
    m.settings.lock().clone()
}

#[tauri::command]
fn set_settings(m: M, s: Settings) {
    m.set_settings(s);
}

#[tauri::command]
async fn analyze(url: String, headers: Option<BTreeMap<String, String>>) -> R<Value> {
    let url = url.trim().to_string();
    let h = headers.unwrap_or_default();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("Geçerli bir bağlantı değil".into());
    }
    let ext = url_ext(&url);
    let direct = (is_file_ext(&ext) || ext == "m3u8" || ext == "mpd") && !is_media_host(&url);
    if direct && ext != "m3u8" && ext != "mpd" {
        let p = http::probe(&url, &h).await.map_err(e)?;
        let cat = category_of(&p.name);
        return Ok(json!({"kind": "http", "http": p, "category": cat}));
    }
    if is_media_host(&url) || ext == "m3u8" || ext == "mpd" {
        let mi = media::analyze(&url, &h).await.map_err(|x| {
            let s = x.to_string();
            s.strip_prefix("TOOLS:").map(|t| format!("TOOLS:{t}")).unwrap_or_else(|| clean_err(&s))
        })?;
        return Ok(json!({"kind": mi.kind.clone(), "media": mi}));
    }
    // tanımadığımız sayfa, yt-dlp ile sayfa tarayıcısını yarıştır
    let (ym, sp) = tokio::join!(
        tokio::time::timeout(std::time::Duration::from_secs(40), media::analyze(&url, &h)),
        scrape::crawl(&url, 0, &h, |_, _| {})
    );
    if let Err(x) = &sp {
        if x.to_string().starts_with("NOTHTML:") {
            let p = http::probe(&url, &h).await.map_err(e)?;
            let cat = category_of(&p.name);
            return Ok(json!({"kind": "http", "http": p, "category": cat}));
        }
    }
    let page = sp.ok();
    if let Ok(Ok(mi)) = ym {
        if !mi.direct && (mi.kind == "playlist" || mi.has_video || mi.audio_abr > 0) {
            return Ok(json!({"kind": mi.kind.clone(), "media": mi, "page": page}));
        }
    }
    match page {
        Some(p) if !p.items.is_empty() => Ok(json!({"kind": "page", "page": p})),
        Some(_) => Err("Bu sayfada indirilebilir bir şey bulunamadı".into()),
        None => Err("Sayfa açılamadı".into()),
    }
}

#[tauri::command]
async fn scrape_page(app: AppHandle, url: String, depth: u32, headers: Option<BTreeMap<String, String>>) -> R<scrape::ScrapeResult> {
    let h = headers.unwrap_or_default();
    let a = app.clone();
    scrape::crawl(&url, depth.min(2), &h, move |pages, found| {
        let _ = a.emit("scrape-progress", json!({"pages": pages, "found": found}));
    })
    .await
    .map_err(e)
}

#[tauri::command]
fn add(m: M, p: AddIn) -> Vec<String> {
    add_one(&m, p)
}

#[tauri::command]
fn add_many(m: M, list: Vec<AddIn>) -> usize {
    list.into_iter().map(|p| add_one(&m, p).len()).sum()
}

#[tauri::command]
fn pause(m: M, id: String) {
    m.pause(&id)
}
#[tauri::command]
fn resume(m: M, id: String) {
    m.resume(&id)
}
#[tauri::command]
fn start_now(m: M, id: String) {
    m.start_now(&id)
}
#[tauri::command]
fn restart(m: M, id: String) {
    m.restart(&id)
}
#[tauri::command]
async fn remove(m: M<'_>, id: String, delete_file: bool) -> R<()> {
    m.remove(&id, delete_file).await;
    Ok(())
}
#[tauri::command]
async fn update_url(m: M<'_>, id: String, url: String) -> R<()> {
    m.update_url(&id, url).await
}

#[tauri::command]
fn free_space(m: M) -> u64 {
    let f = m.settings.lock().folder.clone();
    http::free_space(std::path::Path::new(&f)).unwrap_or(0)
}

/// bitmemişler ya da hepsi, satır başına bir adres
#[tauri::command]
fn export_list(m: M, path: String, all: bool) -> R<usize> {
    let urls: Vec<String> = m
        .tasks
        .lock()
        .iter()
        .filter_map(|t| {
            let d = t.d.lock();
            (all || d.status != model::Status::Done).then(|| d.url.clone())
        })
        .collect();
    std::fs::write(&path, urls.join("\r\n") + "\r\n").map_err(|e| e.to_string())?;
    Ok(urls.len())
}

#[tauri::command]
fn import_list(path: String) -> R<String> {
    let b = std::fs::read(&path).map_err(|e| e.to_string())?;
    if b.len() > 16 << 20 {
        return Err("Dosya çok büyük".into());
    }
    Ok(String::from_utf8_lossy(&b).to_string())
}

#[tauri::command]
fn set_task_limit(m: M, id: String, kb: u64) {
    m.set_task_limit(&id, kb);
}

#[tauri::command]
fn move_task(m: M, id: String, to: usize) {
    m.move_task(&id, to)
}
#[tauri::command]
fn pause_all(m: M) {
    m.pause_all()
}
#[tauri::command]
fn resume_all(m: M) {
    m.resume_all()
}
#[tauri::command]
fn clear_done(m: M) {
    m.clear_done()
}

#[tauri::command]
fn open_task(m: M, id: String) -> R<()> {
    let (p, _) = m.path_of(&id).ok_or("İndirme bulunamadı")?;
    if !std::path::Path::new(&p).exists() {
        return Err("Dosya bulunamadı, taşınmış ya da silinmiş olabilir".into());
    }
    tauri_plugin_opener::open_path(p, None::<&str>).map_err(e)
}

#[tauri::command]
fn reveal_task(m: M, id: String) -> R<()> {
    let (p, folder) = m.path_of(&id).ok_or("İndirme bulunamadı")?;
    if std::path::Path::new(&p).exists() {
        std::process::Command::new("explorer").arg(format!("/select,{}", p.replace('/', "\\"))).spawn().map_err(e)?;
    } else {
        let _ = std::fs::create_dir_all(&folder);
        tauri_plugin_opener::open_path(folder, None::<&str>).map_err(e)?;
    }
    Ok(())
}

#[tauri::command]
fn open_folder(m: M, path: Option<String>) -> R<()> {
    let p = path.unwrap_or_else(|| m.settings.lock().folder.clone());
    let _ = std::fs::create_dir_all(&p);
    tauri_plugin_opener::open_path(p, None::<&str>).map_err(e)
}

#[tauri::command]
fn read_clipboard() -> String {
    arboard::Clipboard::new().and_then(|mut c| c.get_text()).unwrap_or_default()
}

#[tauri::command]
fn write_clipboard(text: String) {
    if let Ok(mut c) = arboard::Clipboard::new() {
        let _ = c.set_text(text);
    }
}

#[tauri::command]
async fn tools_status() -> tools::ToolStatus {
    tools::status().await
}

#[tauri::command]
async fn tools_install(app: AppHandle, which: String) -> R<()> {
    tools::install(app, which).await.map_err(e)
}

#[tauri::command]
async fn verify_hash(m: M<'_>, id: String, expected: String) -> R<Value> {
    let (path, _) = m.path_of(&id).ok_or("İndirme bulunamadı")?;
    let want = expected.trim().to_lowercase();
    let (algo, got) = tokio::task::spawn_blocking(move || hash_file(&path, &want)).await.map_err(e)??;
    Ok(json!({"ok": got == expected.trim().to_lowercase(), "algo": algo, "got": got}))
}

#[tauri::command]
fn cancel_shutdown() {
    let _ = std::process::Command::new("shutdown").arg("/a").creation_flags(NO_WINDOW).spawn();
}

#[tauri::command]
fn ext_status(m: M) -> Value {
    let seen = m.ext_seen.load(std::sync::atomic::Ordering::Relaxed);
    let (version, browser) = m.ext_info.lock().clone();
    json!({"ago": if seen > 0 { model::now() - seen } else { -1 }, "version": version, "browser": browser, "id": native::CHROME_IDS[0], "bridge_error": *m.bridge_err.lock()})
}

#[tauri::command]
fn ext_folder(app: AppHandle) -> String {
    let mut c = vec![];
    if let Ok(r) = app.path().resource_dir() {
        c.push(r.join("extension"));
    }
    c.push(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("extension"));
    let p = c.into_iter().find(|p| p.exists()).unwrap_or_default();
    let p = p.canonicalize().unwrap_or(p).to_string_lossy().trim_start_matches(r"\\?\").to_string();
    let _ = tauri_plugin_opener::open_path(&p, None::<&str>);
    p
}

#[tauri::command]
fn ui_ready(app: AppHandle, m: M) {
    m.ui_ready();
    if !std::env::args().any(|a| a == "--tray") {
        bridge::show(&app);
    }
}

#[tauri::command]
fn close_window(app: AppHandle, m: M) {
    if m.settings.lock().tray {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.hide();
        }
        let first = {
            let mut s = m.settings.lock();
            let f = !s.tray_told;
            s.tray_told = true;
            f
        };
        if first {
            let _ = app.emit("settings-changed", &*m.settings.lock());
            m.save();
            m.os_notify("Trawl arka planda çalışıyor", "İndirmeler sürüyor. Simgeye tıklayınca pencere açılır; kapatmak için sağ tık › Çıkış.");
        }
    } else {
        quit(&app, &m);
    }
}

#[tauri::command]
fn quit_app(app: AppHandle, m: M) {
    quit(&app, &m);
}

fn quit(app: &AppHandle, m: &Manager) {
    m.shutdown();
    std::thread::sleep(std::time::Duration::from_millis(200));
    m.save();
    app.exit(0);
}

fn looks_downloadable(u: &str) -> bool {
    let u = u.trim();
    if !(u.starts_with("http://") || u.starts_with("https://")) || u.len() > 4000 || u.contains(char::is_whitespace) {
        return false;
    }
    let e = url_ext(u);
    is_media_host(u) || is_file_ext(&e) || e == "m3u8" || e == "mpd"
}

// uzantısız bağlantıları yoklamıyoruz, kopyalanan her adrese istek atmak gizliliğe aykırı
fn clip_urls(text: &str) -> Vec<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r#"https?://[^\s<>"'`\x00-\x1f]+"#).unwrap());
    let mut out: Vec<String> = vec![];
    for m in re.find_iter(text) {
        let u = m.as_str().trim_end_matches([')', ']', '}', ',', '.', ';', ':', '!', '?']);
        if looks_downloadable(u) && !out.iter().any(|x| x == u) {
            out.push(u.to_string());
            if out.len() >= 500 {
                break;
            }
        }
    }
    out
}

fn clipboard_watch(m: Arc<Manager>) {
    std::thread::spawn(move || {
        use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;
        let mut last = unsafe { GetClipboardSequenceNumber() };
        let mut last_url = String::new();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(700));
            let s = unsafe { GetClipboardSequenceNumber() };
            if s == last {
                continue;
            }
            last = s;
            if !m.settings.lock().clipboard {
                continue;
            }
            let Ok(t) = arboard::Clipboard::new().and_then(|mut c| c.get_text()) else { continue };
            let urls = clip_urls(&t);
            let key = urls.join("\n");
            if urls.is_empty() || key == last_url {
                continue;
            }
            last_url = key;
            if urls.len() == 1 {
                m.notice(json!({"type": "clip", "url": urls[0]}));
            } else {
                m.notice(json!({"type": "clips", "urls": urls}));
            }
        }
    });
}

fn crash_log() {
    // panic=abort olduğundan iz kalmıyordu, son nefeste dosyaya yaz
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let bt = std::backtrace::Backtrace::force_capture();
        let line = format!("[{}] {info}\n{bt}\n\n", model::now());
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(data_dir().join("crash.log")) {
            use std::io::Write;
            let _ = f.write_all(line.as_bytes());
        }
        prev(info);
    }));
}

pub fn run() {
    crash_log();
    // native messaging host olarak açıldıysak arayüz yok, sadece köprü
    if let Some(origin) = native::host_origin() {
        native::run_host(origin);
        return;
    }
    std::thread::spawn(native::register);
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _| {
            bridge::show(app);
            if let Some(u) = argv.iter().find(|a| a.starts_with("http")) {
                app.state::<Arc<Manager>>().notice(json!({"type": "external", "url": u}));
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--tray"])))
        .setup(|app| {
            let m = Manager::new(app.handle().clone());
            app.manage(m.clone());
            tauri::async_runtime::spawn(bridge::serve(m.clone()));
            clipboard_watch(m.clone());
            tauri::async_runtime::spawn(tools::auto_update(m.clone()));

            let show = MenuItem::with_id(app, "show", "Trawl'ı aç", true, None::<&str>)?;
            let pa = MenuItem::with_id(app, "pause_all", "Hepsini duraklat", true, None::<&str>)?;
            let ra = MenuItem::with_id(app, "resume_all", "Hepsini sürdür", true, None::<&str>)?;
            let q = MenuItem::with_id(app, "quit", "Çıkış", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let menu = Menu::with_items(app, &[&show, &pa, &ra, &sep, &q])?;
            TrayIconBuilder::with_id("tray")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Trawl")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, ev| {
                    let m = app.state::<Arc<Manager>>();
                    match ev.id.as_ref() {
                        "show" => bridge::show(app),
                        "pause_all" => m.pause_all(),
                        "resume_all" => m.resume_all(),
                        "quit" => quit(app, &m),
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, ev| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                        bridge::show(tray.app_handle());
                    }
                })
                .build(app)?;
            let args: Vec<String> = std::env::args().collect();
            if let Some(u) = args.iter().find(|a| a.starts_with("http")).cloned() {
                // arayüz hazır olunca kuyruktan gider
                app.state::<Arc<Manager>>().notice(json!({"type": "external", "url": u}));
            }
            Ok(())
        })
        .on_window_event(|w, ev| {
            if let WindowEvent::CloseRequested { api, .. } = ev {
                let app = w.app_handle();
                let m = app.state::<Arc<Manager>>();
                if m.settings.lock().tray {
                    api.prevent_close();
                    let _ = w.hide();
                } else {
                    m.shutdown();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            snapshot, get_settings, set_settings, analyze, scrape_page, add, add_many, pause, resume, start_now,
            restart, remove, move_task, update_url, free_space, pause_all, resume_all, clear_done, open_task, reveal_task, open_folder,
            read_clipboard, write_clipboard, tools_status, tools_install, cancel_shutdown, ext_folder, ext_status, ui_ready,
            close_window, quit_app, verify_hash, set_task_limit, export_list, import_list
        ])
        .run(tauri::generate_context!())
        .expect("Trawl başlatılamadı");
}

#[cfg(test)]
mod tests {
    #[test]
    fn clipboard_extracts_downloadable_links_from_text() {
        let t = "Kurulum: https://x.com/a/setup.exe, belge (https://y.org/k.pdf). Sayfa https://z.net/hakkinda ve yine https://x.com/a/setup.exe";
        assert_eq!(super::clip_urls(t), ["https://x.com/a/setup.exe", "https://y.org/k.pdf"]);
        assert!(super::clip_urls("düz metin").is_empty());
    }
}
