import { t, locale } from "./i18n.svelte";

// sayı ve süre biçimleri
const nfs: Record<string, Intl.NumberFormat[]> = {};
const nf = (i: 0 | 1) => (nfs[locale()] ??= [0, 1].map((d) => new Intl.NumberFormat(locale(), { minimumFractionDigits: d, maximumFractionDigits: d })))[i];
const U = ["B", "KB", "MB", "GB", "TB"];

/** B ve KB tam sayı, MB ve üstü tek ondalık */
export function bytes(n: number): string {
  if (!n || n < 0) return "0 B";
  let i = 0;
  while (n >= 1024 && i < U.length - 1) { n /= 1024; i++; }
  return `${nf(i < 2 ? 0 : 1).format(n)} ${U[i]}`;
}
/** [değer, birim], birim soluk gösterilsin diye ayrı */
export function bytesParts(n: number): [string, string] {
  const s = bytes(n);
  const k = s.lastIndexOf(" ");
  return [s.slice(0, k), s.slice(k + 1)];
}
export const speed = (n: number) => (n > 0 ? t("{v}/sn", { v: bytes(n) }) : "");

/** kalan süre. 10 sn altı "Birkaç saniye", 1 dk altı 5 sn'ye yuvarla, 1 sa altı dakikayı yukarı yuvarla, sonrası en çok iki birim. -1 hesaplanıyor, -2 bekleniyor */
export function eta(s: number): string {
  if (s === -2) return t("Bekleniyor");
  if (s < 0 || !isFinite(s)) return "";
  if (s < 10) return t("Birkaç saniye");
  if (s < 60) return t("{n} sn", { n: Math.max(10, Math.round(s / 5) * 5) });
  if (s < 3600) return t("{n} dk", { n: Math.ceil(s / 60) });
  const h = Math.floor(s / 3600), m = Math.ceil((s % 3600) / 60);
  if (h >= 48) return t("{n} gün", { n: Math.round(h / 24) });
  return m === 60 ? t("{h} sa", { h: h + 1 }) : m ? t("{h} sa {m} dk", { h, m }) : t("{h} sa", { h });
}
export function dur(s: number): string {
  if (!s) return "";
  s = Math.round(s);
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), x = s % 60;
  return h ? `${h}:${String(m).padStart(2, "0")}:${String(x).padStart(2, "0")}` : `${m}:${String(x).padStart(2, "0")}`;
}
const MONTHS = ["Oca", "Şub", "Mar", "Nis", "May", "Haz", "Tem", "Ağu", "Eyl", "Eki", "Kas", "Ara"];
export function ago(ts: number): string {
  if (!ts) return "";
  const d = Date.now() / 1000 - ts;
  if (d < 60) return t("Az önce");
  if (d < 3600) return t("{n} dk önce", { n: Math.floor(d / 60) });
  if (d < 86400) return t("{n} sa önce", { n: Math.floor(d / 3600) });
  const dt = new Date(ts * 1000);
  if (d < 86400 * 2) return t("Dün {time}", { time: `${String(dt.getHours()).padStart(2, "0")}:${String(dt.getMinutes()).padStart(2, "0")}` });
  const other = dt.getFullYear() !== new Date().getFullYear();
  if (locale() !== "tr-TR") return dt.toLocaleDateString(locale(), { month: "short", day: "numeric", year: other ? "numeric" : undefined });
  return `${dt.getDate()} ${MONTHS[dt.getMonth()]}${other ? " " + dt.getFullYear() : ""}`;
}
export function absTime(ts: number): string {
  return ts ? new Date(ts * 1000).toLocaleString(locale(), { dateStyle: "long", timeStyle: "short" }) : "";
}
export function compact(n: number): string {
  return new Intl.NumberFormat(locale(), { notation: "compact", maximumFractionDigits: 1 }).format(n);
}
/** sayfa seçicideki tür, rust tarafındaki scrape::kind_of_ext ile aynı kalmalı */
export function kindOfExt(e: string): string {
  const K: [string, RegExp][] = [
    ["video", /^(mp4|mkv|webm|avi|mov|wmv|flv|m4v|ts|3gp|mpg|mpeg|ogv)$/],
    ["audio", /^(mp3|m4a|aac|flac|wav|ogg|opus|wma|alac|aiff|mid)$/],
    ["doc", /^(pdf|docx?|xlsx?|pptx?|txt|epub|csv|odt|rtf|mobi|djvu)$/],
    ["app", /^(exe|msi|apk|dmg|deb|rpm|appimage|msix|jar|xapk)$/],
    ["archive", /^(zip|rar|7z|tar|gz|bz2|xz|iso|img|zst|tgz|cab)$/],
    ["image", /^(jpe?g|png|gif|webp|bmp|svg|tiff|avif|heic|ico|psd)$/],
    ["stream", /^(m3u8|mpd)$/],
  ];
  // uzantısız bağlantı pano süzgecinden geçtiyse medya sitesidir, yt-dlp alır
  return K.find(([, re]) => re.test(e))?.[0] || (e ? "other" : "embed");
}

export function host(u: string): string {
  try { return new URL(u).hostname.replace(/^www\./, ""); } catch { return ""; }
}
export function qlabel(h: number): string {
  if (h >= 4320) return "8K";
  if (h >= 2160) return "4K";
  if (h >= 1440) return "2K";
  if (h >= 1080) return "Full HD";
  if (h >= 720) return "HD";
  return "SD";
}

/** ortadan kısalt, uzantı kalsın. genişlik canvas ile ölçülüp önbelleğe alınıyor */
let ctx: CanvasRenderingContext2D | null = null;
const cache = new Map<string, string>();
export function middle(name: string, px: number, font = "500 13px 'Mona Sans Variable'"): string {
  if (!name || px <= 0) return name;
  const key = `${px}|${font}|${name}`;
  const hit = cache.get(key);
  if (hit !== undefined) return hit;
  ctx ??= document.createElement("canvas").getContext("2d");
  if (!ctx) return name;
  ctx.font = font;
  const w = (s: string) => ctx!.measureText(s).width;
  let out = name;
  if (w(name) > px) {
    const dot = name.lastIndexOf(".");
    const ext = dot > 0 && name.length - dot <= 8 ? name.slice(dot) : "";
    const stem = ext ? name.slice(0, dot) : name;
    let lo = 1, hi = stem.length;
    out = "…" + ext;
    while (lo <= hi) {
      const k = (lo + hi) >> 1;
      const head = Math.ceil(k * 0.62), tail = k - head;
      const cand = stem.slice(0, head) + "…" + (tail ? stem.slice(-tail) : "") + ext;
      if (w(cand) <= px) { out = cand; lo = k + 1; } else hi = k - 1;
    }
  }
  if (cache.size > 4000) cache.clear();
  cache.set(key, out);
  return out;
}

/** "dosya[001-100].jpg" aralığını ve yan yana yapıştırılmış adresleri açar */
export function expandBatch(s: string, max = 5000): string[] {
  const out: string[] = [];
  // input kutusu satır sonlarını siliyor, adresler birbirine yapışık gelebilir
  const parts = s.trim().split(/\s+|(?=https?:\/\/)/i).filter((u) => /^https?:\/\//i.test(u));
  for (const u of parts) {
    const m = u.match(/\[(\d+)-(\d+)\]|\[([a-z])-([a-z])\]/i);
    if (!m) { out.push(u); continue; }
    const pre = u.slice(0, m.index!), post = u.slice(m.index! + m[0].length);
    const num = m[1] !== undefined;
    const a = num ? +m[1] : m[3].charCodeAt(0), b = num ? +m[2] : m[4].charCodeAt(0);
    const pad = num && m[1].length > 1 && m[1].startsWith("0") ? m[1].length : 0;
    const dir = a <= b ? 1 : -1;
    for (let i = a; dir > 0 ? i <= b : i >= b; i += dir) {
      if (out.length >= max) return out;
      out.push(pre + (num ? String(i).padStart(pad, "0") : String.fromCharCode(i)) + post);
    }
  }
  return out.slice(0, max);
}
