//! yt-dlp ve ffmpeg'i indirir, günceller

use crate::util::*;
use anyhow::{bail, Result};
use futures_util::StreamExt;
use serde::Serialize;
use std::io::Write;
use tauri::{AppHandle, Emitter};

const YTDLP: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
// LGPL derlemesi, ticari dağıtımda GPL'den rahat
const DENO: &str = "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip";
const FFMPEG: &str = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-lgpl.zip";

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

async fn fetch(app: &AppHandle, name: &str, url: &str) -> Result<Vec<u8>> {
    let r = client().get(url).send().await?.error_for_status()?;
    let total = r.content_length().unwrap_or(0);
    let mut buf = Vec::with_capacity(total as usize);
    let mut st = r.bytes_stream();
    let mut last = std::time::Instant::now();
    while let Some(c) = st.next().await {
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
        let b = fetch(&app, "yt-dlp", YTDLP).await?;
        if b.len() < 1_000_000 {
            bail!("yt-dlp indirilemedi");
        }
        let tmp = dir.join("yt-dlp.exe.new");
        std::fs::File::create(&tmp)?.write_all(&b)?;
        std::fs::rename(tmp, dir.join("yt-dlp.exe"))?;
    }
    if which == "deno" || which == "ytdlp" || which == "all" {
        let b = fetch(&app, "deno", DENO).await?;
        let dir2 = dir.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut z = zip::ZipArchive::new(std::io::Cursor::new(b))?;
            let mut f = z.by_name("deno.exe")?;
            let tmp = dir2.join("deno.exe.new");
            std::io::copy(&mut f, &mut std::fs::File::create(&tmp)?)?;
            std::fs::rename(tmp, dir2.join("deno.exe"))?;
            Ok(())
        })
        .await??;
    }
    if which == "ffmpeg" || which == "all" {
        let b = fetch(&app, "ffmpeg", FFMPEG).await?;
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
                        let mut out = std::fs::File::create(dir2.join(want))?;
                        std::io::copy(&mut f, &mut out)?;
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
