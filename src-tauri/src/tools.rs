//! yt-dlp, ffmpeg ve deno'yu kurar, günceller.
//! eksikler ilk açılışta kendiliğinden kurulur. yt-dlp günde, deno haftada bir güncellenir.
//! ffmpeg her gece derlenen sürüm, sık güncellemek bozar; yalnızca eksikse kurulur.
//! her değişimde eski sürüm saklanır, yenisi açılmazsa ona dönülür.

use crate::util::*;
use anyhow::{bail, Result};
use futures_util::StreamExt;
use serde::Serialize;
use std::path::Path;
use tauri::{AppHandle, Emitter};

const YTDLP: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
// LGPL derlemesi, ticari dağıtımda GPL'den rahat
const DENO: &str = "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip";
const FFMPEG: &str = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-lgpl.zip";
const YTDLP_SUM: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS";
const DENO_SUM: &str = "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip.sha256sum";
const FFMPEG_SUM: &str = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/checksums.sha256";

const DAY: i64 = 86_400;

// elle kurulumla kendiliğinden güncelleme aynı dosyaya aynı anda yazmasın
fn lock() -> &'static tokio::sync::Mutex<()> {
    static L: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    L.get_or_init(Default::default)
}

#[derive(Serialize)]
pub struct ToolStatus {
    pub ytdlp: String,
    pub ytdlp_version: String,
    pub ffmpeg: String,
    pub deno: String,
}

async fn version(p: &Path, arg: &str) -> String {
    let out = tokio::process::Command::new(p).arg(arg).creation_flags(NO_WINDOW).output();
    match tokio::time::timeout(std::time::Duration::from_secs(20), out).await {
        Ok(Ok(o)) if o.status.success() => String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("").trim().to_string(),
        _ => String::new(),
    }
}

pub async fn status() -> ToolStatus {
    let y = find_tool("yt-dlp");
    let ver = match &y {
        Some(p) => version(p, "--version").await,
        None => String::new(),
    };
    ToolStatus {
        ytdlp: y.map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
        ytdlp_version: ver,
        ffmpeg: find_tool("ffmpeg").map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
        deno: find_tool("deno").or_else(|| find_tool("node")).map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
    }
}

/// sağlama dosyasında `file` satırındaki hash, yoksa (deno) ilk hash
fn expected_sha(text: &str, file: &str) -> Option<String> {
    let re = regex::Regex::new(r"\b[0-9a-fA-F]{64}\b").unwrap();
    let hit = |l: &str| re.find(l).map(|m| m.as_str().to_lowercase());
    text.lines().filter(|l| l.contains(file)).find_map(hit).or_else(|| hit(text))
}

/// indirip yayıncının SHA-256 listesiyle karşılaştırır, tutmazsa diske hiçbir şey yazılmaz
async fn fetch_checked(app: &AppHandle, name: &str, url: &str, sums: &str) -> Result<Vec<u8>> {
    use sha2::Digest;
    let file = url.rsplit('/').next().unwrap_or("");
    let txt = client().get(sums).timeout(std::time::Duration::from_secs(30)).send().await?.error_for_status()?.text().await?;
    let Some(want) = expected_sha(&txt, file) else { bail!("{name} için sağlama bilgisi bulunamadı") };
    let b = fetch(app, name, url).await?;
    let got: String = sha2::Sha256::digest(&b).iter().map(|x| format!("{x:02x}")).collect();
    if got != want {
        bail!("{name} doğrulanamadı (SHA-256 tutmuyor). Az sonra yeniden deneyin");
    }
    Ok(b)
}

fn runs(p: &Path, arg: &str) -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new(p).arg(arg).creation_flags(NO_WINDOW).output().map(|o| o.status.success()).unwrap_or(false)
}

/// yenisini yerine koyar, eskisini .old diye saklar. yenisi `check` ile açılmazsa eskisine döner.
/// çalışan exe Windows'ta yeniden adlandırılabiliyor, süren indirme eski kopyayla biter
fn replace(dir: &Path, name: &str, data: &mut dyn std::io::Read, check: &str) -> Result<()> {
    let (target, tmp, old) = (dir.join(name), dir.join(format!("{name}.new")), dir.join(format!("{name}.old")));
    std::io::copy(data, &mut std::fs::File::create(&tmp)?)?;
    let had = target.exists();
    if had {
        let _ = std::fs::remove_file(&old);
        if std::fs::rename(&target, &old).is_err() {
            let _ = std::fs::remove_file(&tmp);
            bail!("{name} şu an kullanımda. Süren medya indirmeleri bitince yeniden deneyin");
        }
    }
    if std::fs::rename(&tmp, &target).is_err() || !runs(&target, check) {
        let _ = std::fs::remove_file(&target);
        let _ = std::fs::remove_file(&tmp);
        if had {
            let _ = std::fs::rename(&old, &target);
            bail!("{name} yeni sürümü açılmadı, eski sürüme dönüldü");
        }
        bail!("{name} kurulamadı, indirilen dosya çalışmıyor");
    }
    Ok(())
}

async fn fetch(app: &AppHandle, name: &str, url: &str) -> Result<Vec<u8>> {
    let r = client().get(url).send().await?.error_for_status()?;
    let total = r.content_length().unwrap_or(0);
    let mut buf = Vec::with_capacity(total as usize);
    let mut st = r.bytes_stream();
    let mut last = std::time::Instant::now();
    loop {
        let Ok(c) = tokio::time::timeout(std::time::Duration::from_secs(60), st.next()).await else { bail!("{name} indirilirken bağlantı takıldı") };
        let Some(c) = c else { break };
        buf.extend_from_slice(&c?);
        if last.elapsed().as_millis() > 120 {
            last = std::time::Instant::now();
            let _ = app.emit("tools-progress", serde_json::json!({"name": name, "done": buf.len(), "total": total}));
        }
    }
    let _ = app.emit("tools-progress", serde_json::json!({"name": name, "done": buf.len(), "total": total}));
    Ok(buf)
}

async fn install_ytdlp(app: &AppHandle) -> Result<()> {
    let b = fetch_checked(app, "yt-dlp", YTDLP, YTDLP_SUM).await?;
    let dir = bin_dir();
    tokio::task::spawn_blocking(move || replace(&dir, "yt-dlp.exe", &mut b.as_slice(), "--version")).await?
}

async fn install_deno(app: &AppHandle) -> Result<()> {
    let b = fetch_checked(app, "deno", DENO, DENO_SUM).await?;
    let dir = bin_dir();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(b))?;
        let mut f = z.by_name("deno.exe")?;
        replace(&dir, "deno.exe", &mut f, "--version")
    })
    .await?
}

async fn install_ffmpeg(app: &AppHandle) -> Result<()> {
    let b = fetch_checked(app, "ffmpeg", FFMPEG, FFMPEG_SUM).await?;
    let _ = app.emit("tools-progress", serde_json::json!({"name": "ffmpeg", "stage": "Açılıyor"}));
    let dir = bin_dir();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(b))?;
        let mut got = 0;
        for i in 0..z.len() {
            let mut f = z.by_index(i)?;
            let n = f.name().to_string();
            for want in ["ffmpeg.exe", "ffprobe.exe"] {
                if n.ends_with(&format!("bin/{want}")) {
                    replace(&dir, want, &mut f, "-version")?;
                    got += 1;
                }
            }
        }
        if got < 2 {
            bail!("ffmpeg paketi beklenen dosyaları içermiyor");
        }
        Ok(())
    })
    .await?
}

/// son sürümün etiketi, api sınırına takılmamak için yönlendirmeden okunuyor
async fn latest_tag(repo: &str) -> Option<String> {
    let u = format!("https://github.com/{repo}/releases/latest");
    let r = client().get(u).timeout(std::time::Duration::from_secs(20)).send().await.ok()?;
    let u = r.url().to_string();
    u.contains("/tag/").then(|| u.rsplit('/').next().unwrap_or("").trim_start_matches('v').to_string())
}

fn ours(name: &str) -> bool {
    find_tool(name).map(|p| p.starts_with(bin_dir())).unwrap_or(false)
}

pub async fn auto_update(m: std::sync::Arc<crate::manager::Manager>) {
    use crate::model::now;
    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
    loop {
        if m.settings.lock().tools_auto_update {
            let _g = lock().lock().await;
            // ilk açılış: eksik olanları sormadan kur
            let mut added = vec![];
            if find_tool("yt-dlp").is_none() && install_ytdlp(&m.app).await.is_ok() {
                added.push("yt-dlp");
            }
            if find_tool("deno").is_none() && find_tool("node").is_none() && install_deno(&m.app).await.is_ok() {
                added.push("deno");
            }
            if find_tool("ffmpeg").is_none() && install_ffmpeg(&m.app).await.is_ok() {
                added.push("ffmpeg");
            }
            if !added.is_empty() {
                m.notice(serde_json::json!({"type": "tools_installed", "names": added}));
            }

            let media_busy = m.tasks.lock().iter().any(|t| t.d.lock().kind == crate::model::Kind::Media && t.running.load(std::sync::atomic::Ordering::SeqCst));
            let (yt_last, deno_last) = {
                let s = m.settings.lock();
                (s.tools_checked, s.deno_checked)
            };
            // sitelerin çoğu yt-dlp eskiyince bozuluyor
            if !media_busy && ours("yt-dlp") && now() - yt_last > DAY {
                let local = status().await.ytdlp_version;
                if let Some(v) = latest_tag("yt-dlp/yt-dlp").await {
                    if v == local || install_ytdlp(&m.app).await.is_ok() {
                        m.settings.lock().tools_checked = now();
                        m.save();
                        if v != local {
                            m.notice(serde_json::json!({"type": "tools_updated", "name": "yt-dlp", "version": v}));
                        }
                    }
                }
            }
            if !media_busy && ours("deno") && now() - deno_last > 7 * DAY {
                let local = match find_tool("deno") {
                    Some(p) => version(&p, "--version").await,
                    None => String::new(),
                };
                if let Some(v) = latest_tag("denoland/deno").await {
                    let same = local.split_whitespace().nth(1) == Some(v.as_str());
                    if same || install_deno(&m.app).await.is_ok() {
                        m.settings.lock().deno_checked = now();
                        m.save();
                    }
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
}

pub async fn install(app: AppHandle, which: String) -> Result<()> {
    let _g = lock().lock().await;
    if which == "ytdlp" || which == "all" {
        install_ytdlp(&app).await?;
    }
    if which == "deno" || which == "ytdlp" || which == "all" {
        install_deno(&app).await?;
    }
    if which == "ffmpeg" || which == "all" {
        install_ffmpeg(&app).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_both_sum_formats() {
        let a = "aaaa  x\nf5389d3698ecde4ef06f4e6b1ce0294fe9ac1396228c8990b8d3416d6d177a7d  yt-dlp.exe\n0000000000000000000000000000000000000000000000000000000000000000  yt-dlp_x86.exe";
        assert_eq!(super::expected_sha(a, "yt-dlp.exe").unwrap(), "f5389d3698ecde4ef06f4e6b1ce0294fe9ac1396228c8990b8d3416d6d177a7d");
        let d = "\r\nAlgorithm : SHA256\r\nHash      : CBA84B26E0DF5AE7F74FC66B86A95B866872A86292EBD84FA1E959927EDD93BF\r\nPath : C:\\a\\deno.zip";
        assert_eq!(super::expected_sha(d, "deno-x86_64-pc-windows-msvc.zip").unwrap(), "cba84b26e0df5ae7f74fc66b86a95b866872a86292ebd84fa1e959927edd93bf");
    }

    /// bozuk yeni sürüm gelirse eskisi yerinde kalmalı
    #[test]
    fn broken_update_rolls_back() {
        let dir = std::env::temp_dir().join(format!("trawl-tools-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let good = std::fs::read(std::env::var("ComSpec").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".into())).unwrap();
        // ilk kurulum çalışan bir exe
        super::replace(&dir, "t.exe", &mut good.as_slice(), "/c").unwrap();
        // bozuk güncelleme reddedilip eskiye dönülmeli
        let e = super::replace(&dir, "t.exe", &mut &b"bozuk"[..], "/c").unwrap_err().to_string();
        assert!(e.contains("eski sürüme dönüldü"), "{e}");
        assert_eq!(std::fs::read(dir.join("t.exe")).unwrap(), good);
        // hiç yokken bozuk gelirse hiçbir şey kalmamalı
        assert!(super::replace(&dir, "u.exe", &mut &b"bozuk"[..], "/c").is_err());
        assert!(!dir.join("u.exe").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
