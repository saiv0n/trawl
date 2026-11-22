//! eklenti köprüsü, 127.0.0.1:47321

use crate::manager::Manager;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use tauri::{Emitter, Manager as _};
use tower_http::cors::{AllowOrigin, CorsLayer};

pub const PORT: u16 = 47321;

// chromium'da kimlik sabit, başka eklenti köprüyü kullanamasın. firefox her kurulumda rastgele uuid veriyor, orada kimlikle kilitlenemiyor
fn ext_origin(o: &str) -> bool {
    let o = o.trim_end_matches('/');
    match o.strip_prefix("chrome-extension://") {
        Some(id) => crate::native::CHROME_IDS.contains(&id),
        None => o.starts_with("moz-extension://"),
    }
}

// eklenti her POST'a Origin ekliyor; Origin'siz olanı (yerel betik vs.) ve web sayfasından geleni reddet
fn allowed(h: &HeaderMap) -> bool {
    h.get("origin").and_then(|v| v.to_str().ok()).map(ext_origin).unwrap_or(false)
}

// GET'te Origin her zaman gelmiyor. özel başlığı web sayfası ön kontrolsüz gönderemez, ön kontrolü de cors reddediyor
fn from_ext(h: &HeaderMap) -> bool {
    allowed(h) || h.contains_key("x-trawl")
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct AddReq {
    url: String,
    referer: String,
    cookies: String,
    filename: String,
    mode: String,
    title: String,
    page: String,
    size: u64,
    /// panelden seçilen kalite, 0 ise ayardaki varsayılan
    height: u32,
    /// eklentinin yakaladığı gerçek istek başlıkları
    headers: BTreeMap<String, String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RefreshRes {
    id: String,
    candidates: Vec<crate::refresh::Cand>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SiteReq {
    host: String,
    skip: bool,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ProbeReq {
    url: String,
    headers: BTreeMap<String, String>,
}

pub fn show(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        // tarayıcı öndeyken windows odak çalmaya izin vermiyor, sadece görev çubuğu yanıp sönüyordu
        #[cfg(windows)]
        if let Ok(h) = w.hwnd() {
            force_front(h.0 as _);
        }
        let _ = w.set_focus();
    }
}

#[cfg(windows)]
fn force_front(h: windows_sys::Win32::Foundation::HWND) {
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    unsafe {
        let fg = GetForegroundWindow();
        if fg == h {
            return;
        }
        let fg_thread = GetWindowThreadProcessId(fg, std::ptr::null_mut());
        let me = GetCurrentThreadId();
        let attached = fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, 1) != 0;
        SetWindowPos(h, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
        SetWindowPos(h, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
        BringWindowToTop(h);
        SetForegroundWindow(h);
        if attached {
            AttachThreadInput(me, fg_thread, 0);
        }
    }
}

async fn ping(State(m): State<Arc<Manager>>, h: HeaderMap, Query(q): Query<BTreeMap<String, String>>) -> Json<Value> {
    // ping'de sürüm ve tarayıcı da geliyor (GET'te Origin her zaman olmuyor)
    if !from_ext(&h) {
        return Json(json!({"app": "Trawl"}));
    }
    if let Some(v) = q.get("v") {
        if let Some(ua) = h.get("user-agent").and_then(|v| v.to_str().ok()) {
            crate::util::set_browser_ua(ua);
        }
        m.ext_seen.store(crate::model::now(), std::sync::atomic::Ordering::Relaxed);
        *m.ext_info.lock() = (v.chars().take(20).collect(), q.get("b").map(|b| b.chars().take(20).collect()).unwrap_or_default());
    }
    let s = m.settings.lock();
    let exts: Vec<String> = s.intercept_exts.split_whitespace().map(|e| e.trim_start_matches('.').to_lowercase()).collect();
    Json(json!({"app": "Trawl", "version": env!("CARGO_PKG_VERSION"), "proto": 2, "intercept": s.intercept, "min_kb": s.min_intercept_kb,
        "exts": exts, "skip": s.intercept_skip, "ask_mb": s.intercept_ask_mb}))
}

async fn add(State(m): State<Arc<Manager>>, h: HeaderMap, Json(r): Json<AddReq>) -> (StatusCode, Json<Value>) {
    if !allowed(&h) || !r.url.starts_with("http") {
        return (StatusCode::FORBIDDEN, Json(json!({"ok": false})));
    }
    let mut hd: BTreeMap<String, String> =
        r.headers.into_iter().filter(|(k, v)| !v.is_empty() && (k == crate::util::BODY_KEY || !crate::util::hop_header(k))).collect();
    if !r.referer.is_empty() && crate::util::hget(&hd, "Referer").is_none() {
        crate::util::hput(&mut hd, "Referer", r.referer.clone());
    }
    if !r.cookies.is_empty() && crate::util::hget(&hd, "Cookie").is_none() {
        crate::util::hput(&mut hd, "Cookie", r.cookies.clone());
    }
    if r.mode == "video" || r.mode == "audio" {
        let s = m.settings.lock().clone();
        let opts = crate::model::MediaOpts {
            mode: r.mode.clone(),
            height: if r.height > 0 { r.height } else { s.default_height },
            container: s.default_container,
            codec: s.default_codec,
            bitrate: s.default_bitrate,
            chapters: true,
            ..Default::default()
        };
        let id = m.add_media(r.url, opts, r.title, String::new(), String::new(), hd, true);
        if m.settings.lock().focus_on_add {
            show(&m.app);
        }
        return (StatusCode::OK, Json(json!({"ok": true, "id": id})));
    }
    if r.mode == "direct" {
        // zaten listedeyse kullanıcı seçsin, açsın ya da yeniden indirsin
        if let Some((id, st, exists)) = m.find_dup(&r.url, &r.filename) {
            let done = st == crate::model::Status::Done;
            if !done || exists {
                m.notice(json!({"type": "dup", "id": id, "done": done, "status": st, "url": r.url, "filename": r.filename, "headers": hd, "page": r.page}));
                show(&m.app);
                return (StatusCode::OK, Json(json!({"ok": true, "id": id, "dup": true})));
            }
        }
        // POST'ta sunucu formu her istekte yeniden işliyor, tek bağlantı daha güvenli
        let conns = if hd.contains_key(crate::util::BODY_KEY) { 1 } else { 0 };
        let id = m.add_http(r.url, r.filename, hd, String::new(), conns, true, r.page);
        if m.settings.lock().focus_on_add {
            show(&m.app);
            m.notice(json!({"type": "added", "id": id}));
        }
        return (StatusCode::OK, Json(json!({"ok": true, "id": id})));
    }
    m.notice(json!({"type": "external", "url": r.url, "headers": hd, "filename": r.filename, "title": r.title, "size": r.size}));
    show(&m.app);
    (StatusCode::OK, Json(json!({"ok": true})))
}

async fn grab(State(m): State<Arc<Manager>>, h: HeaderMap, Json(v): Json<Value>) -> (StatusCode, Json<Value>) {
    if !allowed(&h) {
        return (StatusCode::FORBIDDEN, Json(json!({"ok": false})));
    }
    m.notice(json!({"type": "grab", "data": v}));
    show(&m.app);
    (StatusCode::OK, Json(json!({"ok": true})))
}

async fn probe_stream(h: HeaderMap, Json(r): Json<ProbeReq>) -> (StatusCode, Json<Value>) {
    if !allowed(&h) || !r.url.starts_with("http") {
        return (StatusCode::FORBIDDEN, Json(json!({"ok": false})));
    }
    let hd: BTreeMap<String, String> = r.headers.into_iter().filter(|(k, _)| !crate::util::hop_header(k)).collect();
    match tokio::time::timeout(std::time::Duration::from_secs(30), crate::stream::probe(&r.url, &hd)).await {
        Ok(Ok(si)) => (StatusCode::OK, Json(json!({"ok": true, "info": si}))),
        Ok(Err(e)) => (StatusCode::OK, Json(json!({"ok": false, "error": e.to_string()}))),
        Err(_) => (StatusCode::OK, Json(json!({"ok": false, "error": "zaman aşımı"}))),
    }
}

// popup'taki "bu sitede devralma" anahtarı
async fn site(State(m): State<Arc<Manager>>, h: HeaderMap, Json(r): Json<SiteReq>) -> (StatusCode, Json<Value>) {
    let host = r.host.trim().trim_start_matches("www.").to_lowercase();
    if !allowed(&h) || host.is_empty() || host.contains(['/', ' ']) {
        return (StatusCode::FORBIDDEN, Json(json!({"ok": false})));
    }
    let mut s = m.settings.lock().clone();
    s.intercept_skip.retain(|x| *x != host);
    if r.skip {
        s.intercept_skip.push(host);
    }
    m.set_settings(s.clone());
    let _ = m.app.emit("settings-changed", &s);
    (StatusCode::OK, Json(json!({"ok": true, "skip": s.intercept_skip})))
}

// eklenti bekleyen tazeleme işlerini buradan çekiyor (30 sn'de bir)
async fn jobs(State(m): State<Arc<Manager>>, h: HeaderMap) -> (StatusCode, Json<Value>) {
    if !allowed(&h) {
        return (StatusCode::FORBIDDEN, Json(json!({"ok": false})));
    }
    (StatusCode::OK, Json(json!({"ok": true, "jobs": crate::refresh::take(&m)})))
}

async fn refresh_result(State(m): State<Arc<Manager>>, h: HeaderMap, Json(r): Json<RefreshRes>) -> (StatusCode, Json<Value>) {
    if !allowed(&h) {
        return (StatusCode::FORBIDDEN, Json(json!({"ok": false})));
    }
    let ok = crate::refresh::result(m, r.id, r.candidates).await;
    (StatusCode::OK, Json(json!({"ok": ok})))
}

async fn show_req(State(m): State<Arc<Manager>>, h: HeaderMap) -> StatusCode {
    if !allowed(&h) {
        return StatusCode::FORBIDDEN;
    }
    show(&m.app);
    StatusCode::OK
}

pub async fn serve(m: Arc<Manager>) {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|o: &HeaderValue, _| o.to_str().map(ext_origin).unwrap_or(false)))
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([axum::http::header::CONTENT_TYPE, axum::http::HeaderName::from_static("x-trawl")]);
    let app = Router::new()
        .route("/ping", get(ping))
        .route("/add", post(add))
        .route("/grab", post(grab))
        .route("/probe-stream", post(probe_stream))
        .route("/site", post(site))
        .route("/jobs", post(jobs))
        .route("/refresh-result", post(refresh_result))
        .route("/show", post(show_req))
        .layer(cors)
        .with_state(m.clone());
    // önceki örnek yeni kapanıyorsa port bir an dolu kalabiliyor
    let mut last = String::new();
    for _ in 0..10 {
        match tokio::net::TcpListener::bind(("127.0.0.1", PORT)).await {
            Ok(l) => {
                let _ = axum::serve(l, app).await;
                return;
            }
            Err(e) => last = e.to_string(),
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    let msg = format!("Tarayıcı eklentisi bağlanamıyor: {PORT} numaralı port başka bir program tarafından kullanılıyor ({last}).");
    *m.bridge_err.lock() = msg.clone();
    m.notice(json!({"type": "bridge", "error": msg}));
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_trawl_extension_origin() {
        let id = crate::native::CHROME_IDS[0];
        assert!(super::ext_origin(&format!("chrome-extension://{id}")));
        assert!(super::ext_origin(&format!("chrome-extension://{id}/")));
        assert!(!super::ext_origin("chrome-extension://baskabireklentikimligi"));
        assert!(!super::ext_origin("https://kotu.site"));
        assert!(!super::ext_origin("extension://x"));
    }
}
