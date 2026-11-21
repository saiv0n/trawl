//! yt-dlp ve ffmpeg'i indirir, günceller

use crate::util::*;
use anyhow::{bail, Result};
use futures_util::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

const YTDLP: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
// LGPL derlemesi, ticari dağıtımda GPL'den rahat
const DENO: &str = "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip";
const FFMPEG: &str = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-lgpl.zip";
const YTDLP_SUM: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS";
const DENO_SUM: &str = "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip.sha256sum";
const FFMPEG_SUM: &str = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/checksums.sha256";

#[derive(Serialize)]
pub struct ToolStatus {
    pub ytdlp: String,
    pub ytdlp_version: String,
    pub ffmpeg: String,
    pub deno: String,
}

pub async fn status() -> ToolStatus {
    let y = find_tool("yt-dlp");
    let mut ver = String::new();
    if let Some(p) = &y {
        if let Ok(o) = tokio::process::Command::new(p).arg("--version").creation_flags(NO_WINDOW).output().await {
            ver = String::from_utf8_lossy(&o.stdout).trim().to_string();
        }
    }
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

fn replace(dir: &std::path::Path, name: &str, data: &mut dyn std::io::Read) -> Result<()> {
    let tmp = dir.join(format!("{name}.new"));
    std::io::copy(data, &mut std::fs::File::create(&tmp)?)?;
    if std::fs::rename(&tmp, dir.join(name)).is_err() {
        let _ = std::fs::remove_file(&tmp);
        bail!("{name} şu an kullanımda. Süren medya indirmeleri bitince yeniden deneyin");
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

pub async fn install(app: AppHandle, which: String) -> Result<()> {
    let dir = bin_dir();
    if which == "ytdlp" || which == "all" {
        let b = fetch_checked(&app, "yt-dlp", YTDLP, YTDLP_SUM).await?;
        replace(&dir, "yt-dlp.exe", &mut b.as_slice())?;
    }
    if which == "deno" || which == "ytdlp" || which == "all" {
        let b = fetch_checked(&app, "deno", DENO, DENO_SUM).await?;
        let dir2 = dir.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut z = zip::ZipArchive::new(std::io::Cursor::new(b))?;
            let mut f = z.by_name("deno.exe")?;
            replace(&dir2, "deno.exe", &mut f)
        })
        .await??;
    }
    if which == "ffmpeg" || which == "all" {
        let b = fetch_checked(&app, "ffmpeg", FFMPEG, FFMPEG_SUM).await?;
        let _ = app.emit("tools-progress", serde_json::json!({"name": "ffmpeg", "stage": "Açılıyor"}));
        let dir2 = dir.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut z = zip::ZipArchive::new(std::io::Cursor::new(b))?;
            let mut got = 0;
            for i in 0..z.len() {
                let mut f = z.by_index(i)?;
                let n = f.name().to_string();
                for want in ["ffmpeg.exe", "ffprobe.exe"] {
                    if n.ends_with(&format!("bin/{want}")) {
                        replace(&dir2, want, &mut f)?;
                        got += 1;
                    }
                }
            }
            if got < 2 {
                bail!("ffmpeg paketi beklenen dosyaları içermiyor");
            }
            Ok(())
        })
        .await??;
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
}
