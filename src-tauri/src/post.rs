//! indirme bittikten sonra: internetten geldi işareti, virüs taraması, arşivi açma

use crate::manager::Manager;
use crate::model::{Status, Task};
use crate::util::NoWindow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// linux'ta tarayıcılar kaynağı user.xdg.origin.url özniteliğine yazıyor, dosya yöneticileri bunu gösteriyor
#[cfg(unix)]
pub fn mark_of_web(path: &str, url: &str, page: &str) {
    let ok = |u: &str| (u.starts_with("http://") || u.starts_with("https://")) && !u.contains(['\r', '\n', '\0']);
    let Ok(cp) = std::ffi::CString::new(path) else { return };
    for (k, v) in [("user.xdg.origin.url\0", url), ("user.xdg.referrer.url\0", page)] {
        if ok(v) {
            // tmpfs ya da fat'ta öznitelik yok, olmazsa geç
            unsafe { libc::setxattr(cp.as_ptr(), k.as_ptr().cast(), v.as_ptr().cast(), v.len(), 0) };
        }
    }
}

/// tarayıcıların yazdığı Zone.Identifier akışı. yoksa indirilen exe SmartScreen'e takılmadan açılır
#[cfg(windows)]
pub fn mark_of_web(path: &str, url: &str, page: &str) {
    let clean = |u: &str| if u.starts_with("http://") || u.starts_with("https://") { u.replace(['\r', '\n'], "") } else { String::new() };
    let mut s = String::from("[ZoneTransfer]\r\nZoneId=3\r\n");
    let (u, p) = (clean(url), clean(page));
    if !p.is_empty() {
        s += &format!("ReferrerUrl={p}\r\n");
    }
    if !u.is_empty() {
        s += &format!("HostUrl={u}\r\n");
    }
    // fat32/exfat'ta akış yok, olmazsa geç
    let _ = std::fs::write(format!("{path}:Zone.Identifier"), s);
}

#[cfg(windows)]
fn defender() -> Option<PathBuf> {
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    let p = PathBuf::from(pf).join("Windows Defender").join("MpCmdRun.exe");
    p.exists().then_some(p)
}

/// clamdscan çalışan servisi kullanır, hızlıdır; yoksa clamscan. 0 temiz, 1 tehdit
#[cfg(not(windows))]
pub(crate) async fn scan(path: &str) -> Option<bool> {
    for exe in ["clamdscan", "clamscan"] {
        let Some(p) = crate::util::find_tool(exe) else { continue };
        let mut c = tokio::process::Command::new(p);
        c.args(["--no-summary", "--infected"]);
        if exe == "clamdscan" {
            c.arg("--fdpass");
        }
        let Ok(out) = c.arg("--").arg(path).no_window().output().await else { continue };
        match out.status.code() {
            Some(0) => return Some(true),
            Some(1) => return Some(false),
            _ => continue,
        }
    }
    None
}

/// Some(true) temiz, Some(false) tehdit bulundu, None taranamadı
#[cfg(windows)]
pub(crate) async fn scan(path: &str) -> Option<bool> {
    let exe = defender()?;
    let out = tokio::process::Command::new(exe)
        .args(["-Scan", "-ScanType", "3", "-File", path])
        .no_window()
        .output()
        .await
        .ok()?;
    // gerçek zamanlı koruma dosyayı taramadan önce karantinaya aldıysa mpcmdrun "temiz" diyor
    if !Path::new(path).exists() {
        return Some(false);
    }
    // yönetici değilken tehdit bulsa da çıkış kodu 0 dönüyor, çıktıdaki sayıya bak
    let txt = String::from_utf8_lossy(&out.stdout);
    if let Some(c) = regex::Regex::new(r"found (\d+) threat").unwrap().captures(&txt) {
        return Some(&c[1] == "0");
    }
    match out.status.code() {
        Some(0) => Some(true),
        Some(2) => Some(false),
        _ => None,
    }
}

pub fn is_archive(name: &str) -> bool {
    let n = name.to_lowercase();
    [".zip", ".7z", ".rar", ".tar", ".tar.gz", ".tgz", ".tar.xz", ".tar.bz2"].iter().any(|e| n.ends_with(e))
}

fn extract_dir(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let lower = name.to_lowercase();
    let stem = [".tar.gz", ".tar.xz", ".tar.bz2", ".tgz", ".zip", ".7z", ".rar", ".tar"]
        .iter()
        .find(|e| lower.ends_with(*e))
        .map(|e| &name[..name.len() - e.len()])
        .unwrap_or(&name)
        .to_string();
    let dir = path.parent().unwrap_or(Path::new(".")).join(stem);
    let mut d = dir.clone();
    let mut i = 2;
    while d.exists() {
        d = PathBuf::from(format!("{} ({i})", dir.display()));
        i += 1;
    }
    d
}

/// arşivin içinde tek klasör varsa klasör/klasör diye iç içe kalmasın
fn flatten(dir: &Path) {
    let items: Vec<_> = match std::fs::read_dir(dir) {
        Ok(r) => r.flatten().collect(),
        Err(_) => return,
    };
    if items.len() != 1 || !items[0].path().is_dir() {
        return;
    }
    let tmp = dir.with_extension("trawl-ac");
    if std::fs::rename(items[0].path(), &tmp).is_ok() && std::fs::remove_dir(dir).is_ok() {
        let _ = std::fs::rename(&tmp, dir);
    }
}

/// tar her sıkıştırmayı kendi tanıyor; zip, 7z ve rar için 7z, o yoksa unzip
#[cfg(not(windows))]
pub(crate) async fn extract(path: &str) -> Result<PathBuf, String> {
    use crate::util::find_tool;
    let n = path.to_lowercase();
    let dir = extract_dir(Path::new(path));
    let mut c = if [".tar", ".tar.gz", ".tgz", ".tar.xz", ".tar.bz2"].iter().any(|e| n.ends_with(e)) && find_tool("tar").is_some() {
        let mut c = tokio::process::Command::new("tar");
        c.arg("-xf").arg(path).arg("-C").arg(&dir);
        c
    } else if let Some(z) = find_tool("7z").or_else(|| find_tool("7zz")) {
        let mut c = tokio::process::Command::new(z);
        c.args(["x", "-y", "-bd"]).arg(format!("-o{}", dir.display())).arg("--").arg(path);
        c
    } else if n.ends_with(".zip") && find_tool("unzip").is_some() {
        let mut c = tokio::process::Command::new("unzip");
        c.args(["-q", "-o"]).arg(path).arg("-d").arg(&dir);
        c
    } else {
        return Err("arşivi açacak araç yok, 7zip paketini kurun".into());
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let out = c.stdin(std::process::Stdio::null()).no_window().output().await.map_err(|e| e.to_string())?;
    if !out.status.success() {
        let _ = std::fs::remove_dir_all(&dir);
        let msg = String::from_utf8_lossy(&out.stderr).lines().rfind(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        return Err(if msg.is_empty() { "arşiv açılamadı".into() } else { msg });
    }
    flatten(&dir);
    Ok(dir)
}

/// windows 10'dan beri gelen tar.exe zip'i, windows 11'dekisi 7z ve rar'ı da açıyor
#[cfg(windows)]
pub(crate) async fn extract(path: &str) -> Result<PathBuf, String> {
    let sys = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let tar = PathBuf::from(sys).join("System32").join("tar.exe");
    if !tar.exists() {
        return Err("Windows'ta tar.exe yok".into());
    }
    let dir = extract_dir(Path::new(path));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let out = tokio::process::Command::new(tar)
        .arg("-xf")
        .arg(path)
        .arg("-C")
        .arg(&dir)
        .no_window()
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        let _ = std::fs::remove_dir_all(&dir);
        let msg = String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("").trim().to_string();
        return Err(if msg.is_empty() { "arşiv açılamadı".into() } else { msg });
    }
    flatten(&dir);
    Ok(dir)
}

pub const GONE: &str = "Dosya indikten hemen sonra kayboldu. Antivirüs karantinaya almış olabilir";
#[cfg(windows)]
pub const THREAT: &str = "Windows Defender bu dosyada tehdit buldu, dosya karantinaya alınmış olabilir";
#[cfg(not(windows))]
pub const THREAT: &str = "ClamAV bu dosyada tehdit buldu";

fn stage(t: &Task, s: &str) {
    let mut d = t.d.lock();
    d.status = Status::Processing;
    d.stage = s.into();
}

/// "abc123  dosya.zip" satırlarından bu dosyanınkini, tek satırsa onu al
fn pick_sum(body: &str, name: &str, len: usize) -> Option<String> {
    let re = regex::Regex::new(&format!(r"(?i)\b[0-9a-f]{{{len}}}\b")).ok()?;
    let lines: Vec<&str> = body.lines().filter(|l| re.is_match(l)).collect();
    let line = if lines.len() == 1 { lines[0] } else { lines.iter().find(|l| l.contains(name)).copied()? };
    re.find(line).map(|m| m.as_str().to_lowercase())
}

/// sunucuda dosya.sha256 gibi bir yan dosya varsa getir
pub(crate) async fn sidecar_sum(url: &str, name: &str, h: &std::collections::BTreeMap<String, String>) -> Option<String> {
    if url.contains('?') || url.contains('#') {
        return None;
    }
    for (ext, len) in [("sha256", 64), ("sha256sum", 64), ("sha1", 40), ("md5", 32)] {
        let r = crate::http::rq(&format!("{url}.{ext}"), h).timeout(std::time::Duration::from_secs(8)).send().await;
        let Ok(r) = r else { continue };
        let html = r.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|v| v.contains("html"));
        if !r.status().is_success() || html || r.content_length().unwrap_or(0) > 64 << 10 {
            continue;
        }
        let Ok(body) = r.text().await else { continue };
        if let Some(x) = pick_sum(&body, name, len) {
            return Some(x);
        }
    }
    None
}

/// {path}, {folder}, {name} yerine tırnaklı değerler konur
pub fn expand_cmd(cmd: &str, path: &str) -> String {
    let p = Path::new(path);
    let folder = p.parent().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
    let name = p.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
    #[cfg(windows)]
    let q = |s: &str| format!("\"{s}\"");
    // sh çift tırnak içinde $ ve ` açıyor, tek tırnak güvenli
    #[cfg(not(windows))]
    let q = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    cmd.replace("{path}", &q(path)).replace("{folder}", &q(&folder)).replace("{name}", &q(&name))
}

/// bitti sayılmadan önce çalışır. dosya bozuksa ya da tehdit varsa hata mesajı döner
pub async fn run(m: &Arc<Manager>, t: &Arc<Task>) -> Result<(), String> {
    let (path, url, page, name, headers, http) = {
        let d = t.d.lock();
        let src = if d.src.is_empty() { d.url.clone() } else { d.src.clone() };
        (d.path.clone(), src, d.page.clone(), d.name.clone(), d.headers.clone(), d.kind == crate::model::Kind::Http)
    };
    if path.is_empty() {
        return Ok(());
    }
    if !Path::new(&path).is_file() {
        return Err(GONE.into());
    }
    mark_of_web(&path, &url, &page);
    let s = m.settings.lock().clone();
    if s.auto_hash && http {
        stage(t, "Sağlama aranıyor");
        if let Some(want) = sidecar_sum(&url, &name, &headers).await {
            stage(t, "Sağlama doğrulanıyor");
            let p = path.clone();
            let w = want.clone();
            if let Ok(Ok((algo, got))) = tokio::task::spawn_blocking(move || crate::util::hash_file(&p, &w)).await {
                if got != want {
                    return Err(format!("{algo} tutmuyor, dosya bozuk inmiş olabilir. Baştan indirin"));
                }
                t.d.lock().verified = algo;
            }
        }
    }
    if s.scan {
        stage(t, "Virüs taranıyor");
        if scan(&path).await == Some(false) {
            m.os_notify("Tehdit bulundu", &name);
            return Err(THREAT.into());
        }
    }
    let mut opened = path.clone();
    if s.extract && is_archive(&name) {
        stage(t, "Arşiv açılıyor");
        match extract(&path).await {
            Ok(dir) => {
                m.notice(serde_json::json!({"type": "extracted", "id": t.id(), "name": name, "path": dir.to_string_lossy()}));
                opened = dir.to_string_lossy().to_string();
                if s.extract_delete {
                    let _ = trash::delete(&path);
                }
            }
            Err(e) => m.notice(serde_json::json!({"type": "error", "id": t.id(), "name": name, "error": format!("Arşiv açılamadı: {e}"), "tools": false})),
        }
    }
    if !s.run_cmd.trim().is_empty() {
        #[cfg(windows)]
        let _ = tokio::process::Command::new("cmd").raw_arg(format!("/C {}", expand_cmd(&s.run_cmd, &path))).no_window().spawn();
        #[cfg(not(windows))]
        let _ = tokio::process::Command::new("sh").arg("-c").arg(expand_cmd(&s.run_cmd, &path)).no_window().spawn();
    }
    if s.open_done {
        let _ = tauri_plugin_opener::open_path(&opened, None::<&str>);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn archive_names() {
        assert!(super::is_archive("a.ZIP"));
        assert!(super::is_archive("x.tar.gz"));
        assert!(!super::is_archive("a.exe"));
        let d = super::extract_dir(std::path::Path::new(r"C:\yok\klasor\paket.tar.gz"));
        assert_eq!(d, std::path::PathBuf::from(r"C:\yok\klasor\paket"));
    }

    #[cfg(unix)]
    #[test]
    fn mark_of_web_written() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join(format!("trawl-motw-{}", std::process::id()));
        std::fs::write(&p, b"x").unwrap();
        let ps = p.to_string_lossy().to_string();
        super::mark_of_web(&ps, "https://ornek.test/a.exe", "javascript:alert(1)");
        let c = std::ffi::CString::new(ps.clone()).unwrap();
        let mut buf = [0u8; 256];
        let n = unsafe { libc::getxattr(c.as_ptr(), c"user.xdg.origin.url".as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
        let r = unsafe { libc::getxattr(c.as_ptr(), c"user.xdg.referrer.url".as_ptr(), std::ptr::null_mut(), 0) };
        let _ = std::fs::remove_file(&p);
        assert_eq!(&buf[..n.max(0) as usize], b"https://ornek.test/a.exe");
        assert_eq!(r, -1);
    }

    #[cfg(windows)]
    #[test]
    fn mark_of_web_written() {
        let p = std::env::temp_dir().join(format!("trawl-motw-{}.exe", std::process::id()));
        std::fs::write(&p, b"x").unwrap();
        let ps = p.to_string_lossy().to_string();
        super::mark_of_web(&ps, "https://ornek.test/a.exe", "javascript:alert(1)");
        let z = std::fs::read_to_string(format!("{ps}:Zone.Identifier")).unwrap();
        let _ = std::fs::remove_file(&p);
        assert!(z.contains("ZoneId=3") && z.contains("HostUrl=https://ornek.test/a.exe") && !z.contains("javascript"));
    }

    #[test]
    fn sidecar_parsing() {
        let a = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let b = "a".repeat(64);
        assert_eq!(super::pick_sum(a, "x.iso", 64).as_deref(), Some(a));
        let many = format!("{b}  diger.iso
{a} *x.iso
");
        assert_eq!(super::pick_sum(&many, "x.iso", 64).as_deref(), Some(a));
        assert_eq!(super::pick_sum("<html>yok</html>", "x.iso", 64), None);
        #[cfg(windows)]
        assert_eq!(super::expand_cmd("echo {name} {folder}", r"C:\a b\c.zip"), r#"echo "c.zip" "C:\a b""#);
        #[cfg(not(windows))]
        assert_eq!(super::expand_cmd("echo {name} {folder}", "/a b/it's.zip"), r#"echo 'it'\''s.zip' '/a b'"#);
    }
}
