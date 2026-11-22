# Trawl

A download manager for Windows and Linux. The backend is Rust on Tauri 2, the UI is Svelte 5. It downloads files over several connections at once, grabs videos through yt-dlp, and ships with a browser extension.

## Features

- 1 to 32 connections per file. When a connection finishes early it splits the largest remaining piece and takes half of it
- Piece state is saved to disk every 3 seconds, so a download picks up where it left off even after a crash
- Video and audio from YouTube, TikTok, Instagram, X, Twitch and everything else yt-dlp supports: quality selection, MP4/MKV/WEBM, MP3/M4A/OPUS/FLAC/WAV, embedded subtitles and chapters, SponsorBlock, playlists
- Page grabber that finds video and audio tags, links, srcset, og/twitter meta, JSON-LD, iframes and m3u8/mp4 URLs buried in scripts. It probes file sizes and can follow links one or two levels deep on the same site
- Queue, limit on parallel downloads, global speed limit, sleep/shut down/quit when everything is done
- Scheduler with day selection, HTTP and SOCKS5 proxy, MD5/SHA-1/SHA-256 check on finished files
- Per-download speed limit, batch add with patterns like `file[001-100].jpg`, export and import the list as a text file
- Waits and resumes on its own when the network drops
- Marks downloaded files with their source URL the way browsers do
- Optional virus scan, zip/7z/rar extraction, opening the file or running a command when a download finishes
- If the server has a `file.sha256` (or `.sha1`, `.md5`) next to the file, the result is checked against it
- Connection cap per server and a second speed limit for set hours
- Folder rules by extension or site, per-site username and password (HTTP Basic)
- Browser cookies for video sites, for age-restricted or members-only videos
- Turkish and English UI
- Tray icon, clipboard watching, drag and drop, notifications, start on login, single instance
- Browser extension in `extension/`. It catches media on the page (HLS and DASH too), takes over browser downloads, adds a context menu entry and a download button on YouTube (Shift for video right away, Alt for audio)

## Running from source

```bash
npm install
npx tauri dev
```

To work on the UI alone with fake data, run `npx vite` and open http://localhost:1420.

## Building on Windows

```powershell
npx tauri build --bundles nsis
```

The installer lands in `src-tauri/target/release/bundle/nsis/`.

## Building on Linux

On Ubuntu or Debian:

```bash
sudo apt install libwebkit2gtk-4.1-dev librsvg2-dev libxdo-dev libsoup-3.0-dev libayatana-appindicator3-dev patchelf
npx tauri build --bundles deb,appimage
sudo apt install ./src-tauri/target/release/bundle/deb/Trawl_*_amd64.deb
```

The AppImage ends up in `src-tauri/target/release/bundle/appimage/` and runs without installing.

## Platform differences

| | Windows | Linux |
|---|---|---|
| Data folder | `%APPDATA%\Trawl` | `~/.local/share/Trawl` |
| Cookies and passwords | encrypted with DPAPI | kept in a folder only your user can read |
| Virus scan | Windows Defender | ClamAV (`clamav` package) |
| Archives | built-in `tar.exe` | `tar`, `7z` or `unzip` |
| Source mark on files | `Zone.Identifier` stream | `user.xdg.origin.url` attribute |
| yt-dlp | uses the one on PATH if present | installs its own copy, since distro packages fall behind quickly |

## Components

yt-dlp, ffmpeg (LGPL build) and deno are installed from inside the app under Settings > Components, into the `bin` folder of the data folder. If they are already on PATH, those are used. yt-dlp is checked for updates daily and deno weekly. If a new version fails to start, the old one is put back.

## Portable mode

On Windows, put an empty file named `portable` next to the exe. Settings, the list and components then go into a `data` folder next to the exe instead of `%APPDATA%`.

## Browser extension

The app talks to the extension through native messaging and a local server on `127.0.0.1:47321`. That server only accepts requests from the extension. The native messaging host is registered on every start for Chrome, Chromium, Edge, Brave and Vivaldi, and on Linux also for Firefox, LibreWolf, Waterfox, Floorp and Zen.

Chrome and other Chromium browsers: open `chrome://extensions`, turn on Developer mode, click "Load unpacked" and pick the `extension` folder.

Firefox and LibreWolf: run `npm run ext:build` and use `extension-dist/trawl-firefox-*.zip`. LibreWolf installs it for good once `xpinstall.signatures.required` is turned off in `about:config`. Regular Firefox only accepts unsigned extensions as temporary add-ons from `about:debugging`.

Flatpak builds of these browsers can't start the host from inside their sandbox, so use the deb or the browser's own package.

## Layout

| Path | What it does |
|---|---|
| `src-tauri/src/http.rs` | multi-connection HTTP downloads |
| `src-tauri/src/media.rs` | yt-dlp |
| `src-tauri/src/scrape.rs` | page grabber |
| `src-tauri/src/manager.rs` | queue and saved state |
| `src-tauri/src/bridge.rs` | extension bridge (axum) |
| `src-tauri/src/native.rs` | native messaging host |
| `src-tauri/src/post.rs` | after-download steps: scan, extract, run command |
| `src-tauri/src/tools.rs` | component installer |
| `src/` | UI |

## Notes

- yt-dlp is Unlicense and ffmpeg is the LGPL build, called as a separate program.
- Downloading from YouTube may be against YouTube's terms of service, so it's better not to pitch this as a YouTube downloader.
- Without a code signing certificate, Windows SmartScreen shows a warning on the installer.
