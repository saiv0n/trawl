//! yerel sunucuya karşı gerçek indirme: parçalı indirme, değişen dosya, erken kesilen akış

use crate::http::run_with;
use crate::manager::Limiter;
use crate::model::{Seg, Task, TaskData};
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use std::sync::Arc;

struct Srv {
    data: Vec<u8>,
    etag: String,
}

fn data(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 31 % 251) as u8).collect()
}

async fn file(State(s): State<Arc<Srv>>, h: HeaderMap) -> Response {
    let hv = |k: &str| h.get(k).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let range = hv("range");
    let if_range = hv("if-range");
    let full = || Response::builder().status(200).header("etag", &s.etag).header("content-length", s.data.len()).body(Body::from(s.data.clone())).unwrap();
    if range.is_empty() || (!if_range.is_empty() && if_range != s.etag) {
        return full();
    }
    let (a, b) = range.trim_start_matches("bytes=").split_once('-').unwrap();
    let a: usize = a.parse().unwrap();
    let b: usize = b.parse::<usize>().unwrap().min(s.data.len() - 1);
    Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header("etag", &s.etag)
        .header("content-range", format!("bytes {a}-{b}/{}", s.data.len()))
        .body(Body::from(s.data[a..=b].to_vec()))
        .unwrap()
}

// yoklamada boyut 2000 diyor, asıl istekte 1000 bayt yollayıp kapatıyor
async fn trunc(h: HeaderMap) -> Response {
    if h.contains_key("range") {
        return Response::builder().status(200).header("content-length", 2000).body(Body::from(vec![1u8; 2000])).unwrap();
    }
    let s = futures_util::stream::iter(vec![Ok::<_, std::io::Error>(axum::body::Bytes::from(vec![1u8; 1000]))]);
    Response::builder().status(200).body(Body::from_stream(s)).unwrap()
}

async fn serve(srv: Srv) -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let app = Router::new().route("/f.bin", get(file)).route("/t.bin", get(trunc)).with_state(Arc::new(srv));
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    base
}

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("trawl-e2e-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[tokio::test]
async fn multi_connection_download_is_byte_exact() {
    let want = data(5 << 20);
    let base = serve(Srv { data: want.clone(), etag: "\"v1\"".into() }).await;
    let dir = tmp("multi");
    let t = Arc::new(Task::new(TaskData { url: format!("{base}/f.bin"), folder: dir.to_string_lossy().into(), ..Default::default() }));
    run_with(t.clone(), Arc::new(Limiter::new()), 6).await.unwrap();
    let d = t.d.lock().clone();
    assert_eq!(d.validator, "\"v1\"");
    assert_eq!(std::fs::read(&d.path).unwrap(), want);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn changed_file_is_not_spliced() {
    let size = 3u64 << 20;
    let base = serve(Srv { data: data(size as usize), etag: "\"v2\"".into() }).await;
    let dir = tmp("changed");
    let path = dir.join("f.bin");
    std::fs::File::create(format!("{}.trawl", path.display())).unwrap().set_len(size).unwrap();
    // v1 iken 1000 bayt inmiş gibi
    let t = Arc::new(Task::new(TaskData {
        url: format!("{base}/f.bin"),
        folder: dir.to_string_lossy().into(),
        name: "f.bin".into(),
        path: path.to_string_lossy().into(),
        size,
        done: 1000,
        resumable: true,
        validator: "\"v1\"".into(),
        segs: vec![Seg { s: 0, e: size - 1, p: 1000, busy: false }],
        ..Default::default()
    }));
    let e = run_with(t, Arc::new(Limiter::new()), 4).await.unwrap_err().to_string();
    assert!(e.contains("değişmiş"), "{e}");
    assert!(!path.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn cut_short_single_stream_is_an_error() {
    let base = serve(Srv { data: vec![], etag: String::new() }).await;
    let dir = tmp("trunc");
    let t = Arc::new(Task::new(TaskData { url: format!("{base}/t.bin"), folder: dir.to_string_lossy().into(), ..Default::default() }));
    let e = run_with(t.clone(), Arc::new(Limiter::new()), 4).await.unwrap_err().to_string();
    assert!(e.contains("eksik"), "{e}");
    assert!(!std::path::Path::new(&t.d.lock().path).exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn speed_limit_holds_with_many_connections() {
    let base = serve(Srv { data: data(1 << 20), etag: "\"v1\"".into() }).await;
    let dir = tmp("limit");
    let lim = Arc::new(Limiter::new());
    lim.rate.store(512 << 10, std::sync::atomic::Ordering::Relaxed);
    let t = Arc::new(Task::new(TaskData { url: format!("{base}/f.bin"), folder: dir.to_string_lossy().into(), ..Default::default() }));
    let at = std::time::Instant::now();
    run_with(t, lim, 8).await.unwrap();
    // 1 MB / 512 KB/s, kova başta dolu olduğu için en az ~1 sn
    assert!(at.elapsed().as_secs_f64() > 0.9, "{:?}", at.elapsed());
    let _ = std::fs::remove_dir_all(&dir);
}
