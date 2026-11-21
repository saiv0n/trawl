//! yeni eklenenlerin yerel sunucuya ve gerçek windows araçlarına karşı denemesi

use crate::http::run_with;
use crate::manager::Limiter;
use crate::model::{Login, Task, TaskData};
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

struct Srv {
    data: Vec<u8>,
    /// ilk bu kadar parça isteği hiç veri vermeden kopar
    fail: u32,
    hits: AtomicU32,
    sum: String,
}

fn data(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 7 % 253) as u8).collect()
}

fn range_of(h: &HeaderMap, len: usize) -> Option<(usize, usize)> {
    let r = h.get("range")?.to_str().ok()?;
    let (a, b) = r.trim_start_matches("bytes=").split_once('-')?;
    Some((a.parse().ok()?, b.parse::<usize>().ok()?.min(len - 1)))
}

fn part(s: &Srv, a: usize, b: usize) -> Response {
    Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header("etag", "\"x\"")
        .header("content-range", format!("bytes {a}-{b}/{}", s.data.len()))
        .body(Body::from(s.data[a..=b].to_vec()))
        .unwrap()
}

async fn flaky(State(s): State<Arc<Srv>>, h: HeaderMap) -> Response {
    let (a, b) = range_of(&h, s.data.len()).unwrap();
    // yoklama hep geçsin, sonra gelen parça istekleri kopsun
    if a == 0 && b == 0 {
        return part(&s, 0, 0);
    }
    if s.hits.fetch_add(1, Ordering::SeqCst) < s.fail {
        let st = futures_util::stream::iter(vec![Err::<Bytes, _>(std::io::Error::other("koptu"))]);
        return Response::builder().status(206).header("content-range", format!("bytes {a}-{b}/{}", s.data.len())).body(Body::from_stream(st)).unwrap();
    }
    part(&s, a, b)
}

async fn locked(State(s): State<Arc<Srv>>, h: HeaderMap) -> Response {
    // "ali:gizli"
    if h.get("authorization").and_then(|v| v.to_str().ok()) != Some("Basic YWxpOmdpemxp") {
        return Response::builder().status(401).header("www-authenticate", "Basic realm=\"x\"").body(Body::empty()).unwrap();
    }
    let (a, b) = range_of(&h, s.data.len()).unwrap_or((0, s.data.len() - 1));
    part(&s, a, b)
}

async fn plain(State(s): State<Arc<Srv>>, h: HeaderMap) -> Response {
    let (a, b) = range_of(&h, s.data.len()).unwrap_or((0, s.data.len() - 1));
    part(&s, a, b)
}

/// ikiden fazla eşzamanlı isteği reddetmeden sonsuza kadar bekletir
async fn stingy(State(s): State<Arc<Srv>>, h: HeaderMap) -> Response {
    let (a, b) = range_of(&h, s.data.len()).unwrap();
    if a == 0 && b == 0 {
        return part(&s, 0, 0);
    }
    if s.hits.fetch_add(1, Ordering::SeqCst) >= 2 {
        s.hits.fetch_sub(1, Ordering::SeqCst);
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
    let r = part(&s, a, b);
    s.hits.fetch_sub(1, Ordering::SeqCst);
    r
}

/// php sitelerinin yaptığı gibi dosya adını ham utf-8 yollar
async fn turkish(State(s): State<Arc<Srv>>, h: HeaderMap) -> Response {
    let (a, b) = range_of(&h, s.data.len()).unwrap_or((0, s.data.len() - 1));
    let mut r = part(&s, a, b);
    let v = axum::http::HeaderValue::from_bytes("attachment; filename=\"Türkçe ödev.pdf\"".as_bytes()).unwrap();
    r.headers_mut().insert("content-disposition", v);
    r
}

async fn sum(State(s): State<Arc<Srv>>) -> Response {
    Response::builder().status(200).header("content-type", "text/plain").body(Body::from(s.sum.clone())).unwrap()
}

async fn serve(s: Srv) -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let app = Router::new()
        .route("/flaky.bin", get(flaky))
        .route("/locked.bin", get(locked))
        .route("/f.iso", get(plain))
        .route("/stingy.bin", get(stingy))
        .route("/download.php", get(turkish))
        .route("/f.iso.sha256", get(sum))
        .with_state(Arc::new(s));
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    base
}

fn srv(data: Vec<u8>, fail: u32, sum: String) -> Srv {
    Srv { data, fail, hits: AtomicU32::new(0), sum }
}

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("trawl-e2e2-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn task(url: String, dir: &std::path::Path) -> Arc<Task> {
    Arc::new(Task::new(TaskData { url, folder: dir.to_string_lossy().into(), ..Default::default() }))
}

/// eskiden 10 boş denemede bırakıyordu, şimdi ağ geri gelene kadar beklemeli. iki dakika sürüyor
#[tokio::test]
#[ignore = "yavaş, elle çalıştır"]
async fn survives_long_outage() {
    let want = data(1 << 20);
    let base = serve(srv(want.clone(), 12, String::new())).await;
    let dir = tmp("outage");
    let t = task(format!("{base}/flaky.bin"), &dir);
    run_with(t.clone(), Arc::new(Limiter::new()), 1).await.unwrap();
    assert_eq!(std::fs::read(&t.d.lock().path).unwrap(), want);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn short_outage_with_many_connections() {
    let want = data(4 << 20);
    let base = serve(srv(want.clone(), 4, String::new())).await;
    let dir = tmp("flaky");
    let t = task(format!("{base}/flaky.bin"), &dir);
    run_with(t.clone(), Arc::new(Limiter::new()), 4).await.unwrap();
    assert_eq!(std::fs::read(&t.d.lock().path).unwrap(), want);
    let _ = std::fs::remove_dir_all(&dir);
}

/// eskiden fazla bağlantılar cevapsız kalınca indirme sonsuza dek takılıyordu
#[tokio::test]
async fn silent_connection_limit_does_not_hang() {
    let want = data(4 << 20);
    let base = serve(srv(want.clone(), 0, String::new())).await;
    let dir = tmp("stingy");
    let t = task(format!("{base}/stingy.bin"), &dir);
    tokio::time::timeout(std::time::Duration::from_secs(90), run_with(t.clone(), Arc::new(Limiter::new()), 6)).await.expect("takıldı").unwrap();
    assert_eq!(std::fs::read(&t.d.lock().path).unwrap(), want);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn part_file_is_sparse() {
    use std::os::windows::fs::MetadataExt;
    let want = data(3 << 20);
    let base = serve(srv(want.clone(), 0, String::new())).await;
    let dir = tmp("sparse");
    let t = task(format!("{base}/f.iso"), &dir);
    run_with(t.clone(), Arc::new(Limiter::new()), 4).await.unwrap();
    let p = t.d.lock().path.clone();
    let attrs = std::fs::metadata(&p).unwrap().file_attributes();
    assert_ne!(attrs & 0x200, 0, "seyrek değil: {attrs:#x}");
    assert_eq!(std::fs::read(&p).unwrap(), want);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn login_is_sent_as_basic_auth() {
    let want = data(2 << 20);
    let base = serve(srv(want.clone(), 0, String::new())).await;
    let dir = tmp("auth");
    // önce girişsiz: 401 ölü adres sayılmalı
    let t = task(format!("{base}/locked.bin"), &dir);
    assert!(run_with(t, Arc::new(Limiter::new()), 2).await.is_err());
    crate::util::set_logins(&[Login { host: "127.0.0.1".into(), user: "ali".into(), pass: "gizli".into() }]);
    let t = task(format!("{base}/locked.bin"), &dir);
    let r = run_with(t.clone(), Arc::new(Limiter::new()), 2).await;
    crate::util::set_logins(&[]);
    r.unwrap();
    assert_eq!(std::fs::read(&t.d.lock().path).unwrap(), want);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn task_speed_limit_holds() {
    let base = serve(srv(data(1 << 20), 0, String::new())).await;
    let dir = tmp("tasklim");
    let t = Arc::new(Task::new(TaskData { url: format!("{base}/f.iso"), folder: dir.to_string_lossy().into(), speed_limit: 512, ..Default::default() }));
    let at = std::time::Instant::now();
    run_with(t, Arc::new(Limiter::new()), 6).await.unwrap();
    assert!(at.elapsed().as_secs_f64() > 0.9, "{:?}", at.elapsed());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn sidecar_checksum_found_and_compared() {
    use sha2::Digest;
    let want = data(1 << 20);
    let hex: String = sha2::Sha256::digest(&want).iter().map(|x| format!("{x:02x}")).collect();
    let base = serve(srv(want.clone(), 0, format!("{hex}  f.iso\n"))).await;
    let got = crate::post::sidecar_sum(&format!("{base}/f.iso"), "f.iso", &Default::default()).await;
    assert_eq!(got.as_deref(), Some(hex.as_str()));
    // yan dosyası olmayan
    assert_eq!(crate::post::sidecar_sum(&format!("{base}/flaky.bin"), "flaky.bin", &Default::default()).await, None);
    // imzalı adreste denemiyor
    assert_eq!(crate::post::sidecar_sum(&format!("{base}/f.iso?x=1"), "f.iso", &Default::default()).await, None);
}

#[test]
fn host_share_splits_connections() {
    use crate::http::{HostShare, HOST_CONNS};
    let old = HOST_CONNS.swap(3, Ordering::Relaxed);
    let a = HostShare::take("pay.test".into(), 8);
    let b = HostShare::take("pay.test".into(), 8);
    assert_eq!((a.n, b.n), (3, 1));
    drop(a);
    let c = HostShare::take("pay.test".into(), 8);
    assert_eq!(c.n, 2);
    HOST_CONNS.store(old, Ordering::Relaxed);
}

#[tokio::test]
async fn windows_tar_extracts_zip() {
    let dir = tmp("zip");
    std::fs::write(dir.join("a.txt"), b"merhaba").unwrap();
    let st = std::process::Command::new(r"C:\Windows\System32\tar.exe").current_dir(&dir).args(["-a", "-cf", "p.zip", "a.txt"]).status().unwrap();
    assert!(st.success());
    std::fs::remove_file(dir.join("a.txt")).unwrap();
    let out = crate::post::extract(&dir.join("p.zip").to_string_lossy()).await.unwrap();
    assert_eq!(out, dir.join("p"));
    assert_eq!(std::fs::read(out.join("a.txt")).unwrap(), b"merhaba");
    // ikinci kez açınca üstüne yazmasın
    let out2 = crate::post::extract(&dir.join("p.zip").to_string_lossy()).await.unwrap();
    assert_eq!(out2, dir.join("p (2)"));
    // içinde tek klasör olan arşiv iç içe açılmasın
    std::fs::create_dir_all(dir.join("paket").join("alt")).unwrap();
    std::fs::write(dir.join("paket").join("alt").join("b.txt"), b"x").unwrap();
    let st = std::process::Command::new(r"C:\Windows\System32\tar.exe").current_dir(&dir).args(["-a", "-cf", "paket.zip", "paket"]).status().unwrap();
    assert!(st.success());
    std::fs::remove_dir_all(dir.join("paket")).unwrap();
    let out3 = crate::post::extract(&dir.join("paket.zip").to_string_lossy()).await.unwrap();
    assert!(out3.join("alt").join("b.txt").exists(), "iç içe açıldı");
    // bozuk arşiv hata versin, boş klasör kalmasın
    std::fs::write(dir.join("bozuk.zip"), b"zip degil").unwrap();
    assert!(crate::post::extract(&dir.join("bozuk.zip").to_string_lossy()).await.is_err());
    assert!(!dir.join("bozuk").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn defender_scans_clean_file() {
    let dir = tmp("scan");
    let p = dir.join("temiz.txt");
    std::fs::write(&p, b"temiz bir dosya").unwrap();
    let r = crate::post::scan(&p.to_string_lossy()).await;
    eprintln!("defender: {r:?}");
    // defender kapalıysa ya da başka antivirüs varsa None olabilir, ama tehdit demesin
    assert_ne!(r, Some(false));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rules_pick_folder() {
    use crate::model::Rule;
    let r = Rule { exts: "zip rar".into(), host: "github.com".into(), folder: r"D:\Kod".into() };
    assert!(r.hits("a.zip", "https://codeload.github.com/x/a.zip"));
    assert!(!r.hits("a.zip", "https://gitlab.com/a.zip"));
    assert!(!r.hits("a.exe", "https://github.com/a.exe"));
    let any_zip = Rule { exts: ".ZIP".into(), host: String::new(), folder: r"D:\Z".into() };
    assert!(any_zip.hits("A.Zip", "https://x.test/a"));
    // klasörsüz ya da koşulsuz kural hiçbir şeyi tutmaz
    assert!(!Rule { exts: "zip".into(), host: String::new(), folder: String::new() }.hits("a.zip", "https://x.test"));
    assert!(!Rule { exts: String::new(), host: String::new(), folder: "D:\\".into() }.hits("a.zip", "https://x.test"));
}

#[tokio::test]
async fn raw_utf8_filename_is_kept() {
    let base = serve(srv(data(1 << 20), 0, String::new())).await;
    let p = crate::http::probe(&format!("{base}/download.php"), &Default::default()).await.unwrap();
    assert_eq!(p.name, "Türkçe ödev.pdf");
}

#[test]
fn server_errors_read_like_sentences() {
    use crate::util::clean_err;
    assert!(clean_err("Sunucu 401 döndü").contains("şifre"));
    assert!(clean_err("Sunucu 403 döndü").contains("403"));
    assert!(clean_err("Sunucu 503 döndü").contains("hata veriyor"));
    assert!(clean_err("ERROR: [generic] Got HTTP Error 403 caused by Cloudflare anti-bot challenge").contains("bot"));
    assert!(clean_err("ERROR: [instagram] abc: login required").contains("oturum"));
}

#[tokio::test]
async fn run_command_after_download() {
    let dir = tmp("cmd");
    let f = dir.join("biten dosya.txt");
    std::fs::write(&f, b"x").unwrap();
    let cmd = crate::post::expand_cmd("copy {path} {folder}\\kopya.txt", &f.to_string_lossy());
    let st = tokio::process::Command::new("cmd").raw_arg(format!("/C {cmd}")).status().await.unwrap();
    assert!(st.success());
    assert!(dir.join("kopya.txt").exists(), "boşluklu yolda komut çalışmadı");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn browser_cookies_become_a_cookie_file() {
    let p = crate::media::cookie_file("https://www.instagram.com/p/x/", "sessionid=abc; csrftoken=d=e; bozuk").unwrap();
    let txt = std::fs::read_to_string(&p).unwrap();
    let _ = std::fs::remove_file(&p);
    assert!(txt.starts_with("# Netscape HTTP Cookie File"));
    assert!(txt.contains(".instagram.com	TRUE	/	TRUE	0	sessionid	abc"));
    assert!(txt.contains("csrftoken	d=e"));
    assert!(!txt.contains("bozuk"));
}

/// eicar sahte virüs dosyası, defender'ın tehdit bulduğu yolu dener. windows bildirim gösterir
#[tokio::test]
#[ignore = "defender uyarısı çıkarır, elle çalıştır"]
async fn defender_flags_eicar() {
    let dir = tmp("eicar");
    let p = dir.join("eicar.com");
    let body = ["X5O!P%@AP[4\\PZX54(P^)7CC)7}$", "EICAR-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*"].concat();
    let _ = std::fs::write(&p, body);
    let r = crate::post::scan(&p.to_string_lossy()).await;
    eprintln!("defender eicar: {r:?}, dosya duruyor mu: {}", p.exists());
    let _ = std::fs::remove_dir_all(&dir);
    assert_ne!(r, Some(true));
}
