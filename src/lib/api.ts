// Tauri köprüsü. vite dev ile tarayıcıda açınca sahte veriyle çalışıyor
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
export const isWin = typeof navigator === "undefined" || !/Linux|X11|Macintosh/.test(navigator.userAgent);
export const sep = isWin ? "\\" : "/";

export type Status = "queued" | "probing" | "downloading" | "processing" | "paused" | "done" | "error";
export interface Task {
  id: string; kind: "http" | "media"; url: string; page: string; name: string; title: string; folder: string; path: string;
  size: number; done: number; status: Status; error: string; category: string; thumb: string; created: number;
  finished: number; speed: number; eta: number; conns: number; segs: [number, number, number, number][]; stage: string;
  resumable: boolean; mode: string; quality: string; speed_limit: number; verified: string;
}
export interface Snapshot { tasks: Task[]; speed: number; history: number[]; session: number; eta_total: number; ids?: string[] }
export interface Settings {
  folder: string; connections: number; concurrent: number; speed_limit: number; categorize: boolean; clipboard: boolean;
  tray: boolean; sound: boolean; notify: boolean; after_all: string; auto_resume: boolean; default_height: number;
  default_container: string; default_codec: string; default_bitrate: number; intercept: boolean; min_intercept_kb: number; intercept_exts: string; intercept_skip: string[]; intercept_ask_mb: number;
  accent: string; theme: string; motion: boolean; motion_mode: string; density: string; clipboard_asked: boolean; tray_told: boolean; autostart: boolean;
  tools_auto_update: boolean; tools_checked: number; schedule: boolean; schedule_start: string; schedule_stop: string; schedule_days: number[]; scan: boolean; extract: boolean; proxy: string;
  extract_delete: boolean; host_conns: number; alt_limit: boolean; alt_speed: number; alt_start: string; alt_stop: string;
  rules: { exts: string; host: string; folder: string }[]; logins: { host: string; user: string; pass: string }[];
  cookies_from: string; open_done: boolean; run_cmd: string; auto_hash: boolean; lang: string; focus_on_add: boolean;
}

export async function call<T = any>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (inTauri) return invoke<T>(cmd, args);
  return mock(cmd, args) as T;
}

export async function on<T = any>(ev: string, fn: (p: T) => void): Promise<() => void> {
  if (inTauri) return listen<T>(ev, (e) => fn(e.payload));
  mockListeners[ev] = [...(mockListeners[ev] || []), fn];
  return () => {};
}

// önizleme verisi
const mockListeners: Record<string, ((p: any) => void)[]> = {};
const now = Date.now() / 1000;
// ?demo tanıtım kaydı için: ingilizce, linux yolları, kendi küçük resimleri, biten indirme gerçekten biter
const qs = new URLSearchParams(typeof location !== "undefined" ? location.search : "");
const demoMode = qs.has("demo");
const HOME = demoMode ? "/home/you/Downloads/Trawl" : "C:\\Users\\me\\Downloads\\Trawl";
const art = (a: string, b: string, c: string) =>
  "data:image/svg+xml," + encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 320 180"><defs><linearGradient id="s" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#05070f"/><stop offset="1" stop-color="${c}"/></linearGradient><filter id="b"><feGaussianBlur stdDeviation="9"/></filter></defs><rect width="320" height="180" fill="url(#s)"/><g filter="url(#b)" opacity=".9"><path d="M-20 110 C 60 40, 120 120, 190 60 S 300 30, 340 70" stroke="${a}" stroke-width="26" fill="none"/><path d="M-20 80 C 80 20, 150 90, 230 40 S 320 20, 340 30" stroke="${b}" stroke-width="14" fill="none" opacity=".7"/></g><g fill="#fff" opacity=".7"><circle cx="40" cy="22" r=".9"/><circle cx="110" cy="14" r=".7"/><circle cx="200" cy="26" r="1"/><circle cx="270" cy="12" r=".8"/><circle cx="300" cy="44" r=".6"/><circle cx="160" cy="36" r=".6"/></g><path d="M0 180 L0 140 L40 118 L70 132 L110 100 L150 128 L190 108 L230 134 L270 112 L320 138 L320 180Z" fill="#03040a"/></svg>`);
const mk = (o: Partial<Task>): Task => ({
  id: Math.random().toString(36).slice(2, 10), kind: "http", url: "https://example.com/file", page: "", name: "dosya", title: "",
  folder: HOME, path: "", size: 0, done: 0, status: "downloading", error: "", category: "diger",
  thumb: "", created: now, finished: 0, speed: 0, eta: -1, conns: 0, segs: [], stage: "", resumable: true, mode: "", quality: "", speed_limit: 0, verified: "", ...o,
});
function segs(n: number, prog: number): [number, number, number, number][] {
  return Array.from({ length: n }, (_, i) => [i / n, (i + 1) / n, Math.min(1, Math.max(0, prog + Math.sin(i * 1.7) * 0.25)), prog < 1 ? 1 : 0]);
}
const demo: Task[] = demoMode ? [
  mk({ kind: "media", name: "Northern Lights, 4K HDR timelapse", title: "Northern Lights, 4K HDR timelapse", url: "https://vimeo.com/aurora", category: "video", size: 2_140_000_000, done: 820_000_000, speed: 41_000_000, eta: 32, conns: 8, stage: "Video", thumb: art("#3dffb0", "#7c5cff", "#0b2a3a"), quality: "2160p MP4", mode: "video" }),
  mk({ name: "ubuntu-26.04-desktop-amd64.iso", url: "https://releases.ubuntu.com/26.04/ubuntu-26.04-desktop-amd64.iso", category: "arsiv", size: 6_400_000_000, done: 2_500_000_000, speed: 88_000_000, eta: 44, conns: 16, segs: segs(16, 0.39) }),
  mk({ name: "blender-5.0-linux-x64.tar.xz", url: "https://download.blender.org/release/blender-5.0-linux-x64.tar.xz", category: "program", size: 410_000_000, done: 352_000_000, speed: 9_000_000, eta: 6, conns: 8, segs: segs(8, 0.86) }),
  mk({ kind: "media", name: "Rainy Night Jazz.opus", url: "https://soundcloud.com/rainy", category: "muzik", size: 64_000_000, done: 64_000_000, status: "processing", stage: "Kapak ekleniyor", thumb: art("#ffb86b", "#ff5c8a", "#2a0f1f"), quality: "OPUS 160k", mode: "audio" }),
  mk({ name: "fedora-workstation-43-x86_64.iso", url: "https://download.fedoraproject.org/fedora-43.iso", category: "arsiv", size: 2_600_000_000, done: 910_000_000, status: "paused", segs: segs(8, 0.35) }),
  mk({ name: "Q3-Report-2026.pdf", url: "https://example.com/Q3-Report-2026.pdf", category: "belge", size: 14_200_000, done: 14_200_000, status: "done", finished: now - 300, path: HOME + "/Documents/Q3-Report-2026.pdf", verified: "SHA-256" }),
  mk({ name: "wallpaper-fjord-8k.png", url: "https://example.com/wallpaper-fjord-8k.png", category: "resim", size: 48_000_000, done: 0, status: "queued" }),
] : [
  mk({ kind: "media", name: "Synthwave Drive — 4K Night City Mix", title: "Synthwave Drive — 4K Night City Mix", category: "video", size: 1_842_000_000, done: 1_120_000_000, speed: 38_400_000, eta: 19, conns: 8, stage: "Video", thumb: "https://i.ytimg.com/vi/4xDzrJKXOOY/hqdefault.jpg", quality: "2160p MP4", mode: "video" }),
  mk({ name: "ubuntu-24.04.1-desktop-amd64.iso", category: "arsiv", size: 6_114_656_256, done: 2_400_000_000, speed: 62_000_000, eta: 60, conns: 16, segs: segs(16, 0.4) }),
  mk({ kind: "media", name: "Lo-fi Beats to Code To.mp3", category: "muzik", size: 92_000_000, done: 92_000_000, status: "processing", stage: "Kapak ekleniyor", thumb: "https://i.ytimg.com/vi/jfKfPfyJRdk/hqdefault.jpg", quality: "MP3 320k", mode: "audio" }),
  mk({ name: "Blender-4.3-windows-x64.msi", category: "program", size: 380_000_000, done: 150_000_000, status: "paused", segs: segs(8, 0.39) }),
  mk({ name: "Annual-Report-2026.pdf", category: "belge", size: 14_200_000, done: 14_200_000, status: "done", finished: now - 300, path: "C:/Users/me/Downloads/Trawl/Annual-Report-2026.pdf" }),
  mk({ name: "dataset-images.zip", category: "arsiv", size: 0, done: 0, status: "error", error: "Bağlantının süresi dolmuş olabilir (403)" }),
  mk({ name: "wallpaper-aurora-8k.png", category: "resim", size: 48_000_000, done: 0, status: "queued" }),
];
if (demoMode && typeof window !== "undefined") (window as any).__demo = demo;
// ?n=500 ile çok satırda dene
const bulkN = Number(new URLSearchParams(typeof location !== "undefined" ? location.search : "").get("n") || 0);
const bulk: Task[] = Array.from({ length: bulkN }, (_, i) =>
  mk({ name: `dataset-part-${String(i + 1).padStart(4, "0")}.bin`, size: 512_000_000, done: (i * 37_000_000) % 512_000_000, speed: i % 3 ? 2_400_000 : 0, status: i % 3 ? "downloading" : "queued", eta: 120 + i, conns: 8, segs: segs(8, ((i * 37) % 512) / 512) }),
);
let hist = Array.from({ length: 120 }, (_, i) => 40e6 + Math.sin(i / 6) * 25e6 + Math.random() * 15e6);
function mock(cmd: string, args?: any): any {
  switch (cmd) {
    case "snapshot":
      for (const t of demo.concat(bulk)) if (t.status === "downloading") {
        t.done = Math.min(t.size, t.done + t.speed / 4); t.speed *= 0.9 + Math.random() * 0.2; if (t.segs.length) t.segs = segs(t.segs.length, t.done / t.size);
        if (demoMode) { t.eta = Math.max(0, Math.round((t.size - t.done) / Math.max(t.speed, 1))); if (t.done >= t.size) Object.assign(t, { status: "done", finished: Date.now() / 1000, speed: 0, conns: 0, segs: [], path: `${HOME}/${t.name}` }); }
      }
      hist = [...hist.slice(1), demo.filter((t) => t.status === "downloading").reduce((a, t) => a + t.speed, 0)];
      if (new URLSearchParams(location.search).has("empty")) return { tasks: [], speed: 0, history: hist.map(() => 0), session: 0, eta_total: -1 };
      return { tasks: demo.concat(bulk), speed: hist[hist.length - 1], history: hist, session: 12_400_000_000, eta_total: 74 };
    case "get_settings":
      return { folder: HOME, connections: 8, concurrent: 3, speed_limit: 0, categorize: true, clipboard: true, tray: true, sound: true, notify: true, after_all: "none", auto_resume: true, default_height: 1080, default_container: "mp4", default_codec: "mp3", default_bitrate: 320, intercept: true, min_intercept_kb: 512, intercept_exts: "zip rar 7z exe msi iso mp4 mkv mp3 pdf", intercept_skip: ["drive.google.com"], intercept_ask_mb: 50, accent: "cobalt", theme: new URLSearchParams(location.search).get("theme") || "light", motion: true, motion_mode: "system", density: "default", clipboard_asked: false, tray_told: false, autostart: false, tools_auto_update: true, tools_checked: 0, schedule: false, schedule_start: "02:00", schedule_stop: "08:00", schedule_days: [0, 1, 2, 3, 4, 5, 6], scan: false, extract: false, proxy: "", extract_delete: false, host_conns: 16, alt_limit: false, alt_speed: 1024, alt_start: "09:00", alt_stop: "18:00", rules: [], logins: [], cookies_from: "", open_done: false, run_cmd: "", auto_hash: true, lang: qs.get("lang") || "tr", focus_on_add: true };
    case "tools_status":
      return { ytdlp: "C:\\bin\\yt-dlp.exe", ytdlp_version: "2026.10.26", ffmpeg: "C:\\bin\\ffmpeg.exe" };
    case "analyze":
      if (demoMode) return new Promise((res) => setTimeout(() => res({
        kind: "media", media: { kind: "media", url: args.url, title: "Fjords from Above, 8K drone film", thumb: art("#5ad1ff", "#3dffb0", "#08223a"), duration: 1342, uploader: "Nordic Aerials", extractor: "Vimeo", views: 1_260_000, upload_date: "20260918", has_video: true, audio_abr: 160, audio_size: 26_000_000, subs: ["en", "no", "de"], entries: [], direct: false,
          videos: [{ height: 4320, size: 5_900_000_000, fps: 30, hdr: true, codec: "av01" }, { height: 2160, size: 2_100_000_000, fps: 60, hdr: true, codec: "vp9" }, { height: 1440, size: 1_050_000_000, fps: 60, hdr: false, codec: "vp9" }, { height: 1080, size: 540_000_000, fps: 60, hdr: false, codec: "avc1" }, { height: 720, size: 260_000_000, fps: 30, hdr: false, codec: "avc1" }] },
        page: { items: [] },
      }), 1100));
      return new Promise((res) => setTimeout(() => res({
        kind: "media", media: { kind: "media", url: args.url, title: "Synthwave Drive — 4K Night City Mix", thumb: "https://i.ytimg.com/vi/4xDzrJKXOOY/maxresdefault.jpg", duration: 3725, uploader: "Neon Waves", extractor: "Youtube", views: 2840000, upload_date: "20260412", has_video: true, audio_abr: 160, audio_size: 71_000_000, subs: ["en", "tr", "de"], entries: [], direct: false,
          videos: [{ height: 2160, size: 1_840_000_000, fps: 60, hdr: true, codec: "vp9" }, { height: 1440, size: 980_000_000, fps: 60, hdr: false, codec: "vp9" }, { height: 1080, size: 520_000_000, fps: 60, hdr: false, codec: "avc1" }, { height: 720, size: 260_000_000, fps: 30, hdr: false, codec: "avc1" }, { height: 480, size: 120_000_000, fps: 30, hdr: false, codec: "avc1" }, { height: 360, size: 70_000_000, fps: 30, hdr: false, codec: "avc1" }] },
        page: { items: Array(14).fill(0) },
      }), 1400));
    case "scrape_page":
      return new Promise((res) => setTimeout(() => res({ url: args.url, title: "Örnek sayfa", icon: "", pages: 1, items: [
        { url: "https://cdn.example.com/hls/master.m3u8", kind: "stream", name: "master.m3u8", ext: "m3u8", size: 0, mime: "", source: "script", thumb: "", title: "Canlı yayın", page: args.url },
        { url: "https://cdn.example.com/v/trailer-1080.mp4", kind: "video", name: "trailer-1080.mp4", ext: "mp4", size: 148_000_000, mime: "video/mp4", source: "video", thumb: "", title: "Fragman", page: args.url },
        { url: "https://cdn.example.com/a/theme.mp3", kind: "audio", name: "theme.mp3", ext: "mp3", size: 8_400_000, mime: "audio/mpeg", source: "audio", thumb: "", title: "Ana tema", page: args.url },
        { url: "https://www.youtube.com/embed/abc", kind: "embed", name: "youtube.com", ext: "", size: 0, mime: "", source: "iframe", thumb: "", title: "Gömülü video", page: args.url },
        { url: "https://cdn.example.com/files/presskit.zip", kind: "archive", name: "presskit.zip", ext: "zip", size: 212_000_000, mime: "", source: "a", thumb: "", title: "Basın kiti", page: args.url },
        { url: "https://cdn.example.com/docs/manual.pdf", kind: "doc", name: "manual.pdf", ext: "pdf", size: 3_100_000, mime: "", source: "a", thumb: "", title: "", page: args.url },
        ...Array.from({ length: 8 }, (_, i) => ({ url: `https://picsum.photos/id/${10 + i}/1600/900`, kind: "image", name: `photo-${i + 1}.jpg`, ext: "jpg", size: 400_000 + i * 90_000, mime: "image/jpeg", source: "img", thumb: "", title: "", page: args.url })),
      ] }), 900));
    case "add": {
      if (!demoMode) return null;
      const p = args.p, h = p.opts?.height || 2160;
      const size = p.kind === "media" ? ({ 4320: 5_900_000_000, 2160: 2_100_000_000, 1440: 1_050_000_000, 1080: 540_000_000 } as Record<number, number>)[h] || 260_000_000 : 300_000_000;
      demo.unshift(mk({ kind: p.kind, name: p.title || p.name || "download", title: p.title || "", url: p.url, thumb: p.thumb || "", category: p.kind === "media" ? "video" : "diger", size, done: 0, speed: 96_000_000, conns: 16, mode: p.opts?.mode || "", quality: p.kind === "media" ? `${h}p MP4` : "", stage: p.kind === "media" ? "Video" : "", created: Date.now() / 1000 }));
      return demo[0].id;
    }
    case "ext_status":
      return { ago: 12, version: "1.5.0", browser: "edge", id: "elnkcfkeoeihikkacnjfadeppelgghjl" };
    case "free_space":
      return 214_000_000_000;
    case "update_url":
      return new Promise((res) => setTimeout(res, 600));
    case "verify_hash": {
      const n = args.expected.trim().length;
      if (![32, 40, 64].includes(n)) return Promise.reject("MD5, SHA-1 ya da SHA-256 değeri yapıştırın");
      return new Promise((res) => setTimeout(() => res({ ok: !args.expected.startsWith("0"), algo: n === 32 ? "MD5" : n === 40 ? "SHA-1" : "SHA-256", got: "" }), 500));
    }
    default:
      return null;
  }
}
export function mockEmit(ev: string, p: any) {
  for (const f of mockListeners[ev] || []) f(p);
}
