// Türkçe sayı ve süre biçimleri
const nf1 = new Intl.NumberFormat("tr-TR", { minimumFractionDigits: 1, maximumFractionDigits: 1 });
const nf0 = new Intl.NumberFormat("tr-TR", { maximumFractionDigits: 0 });
const U = ["B", "KB", "MB", "GB", "TB"];

/** B ve KB tam sayı, MB ve üstü tek ondalık */
export function bytes(n: number): string {
  if (!n || n < 0) return "0 B";
  let i = 0;
  while (n >= 1024 && i < U.length - 1) { n /= 1024; i++; }
  return `${i < 2 ? nf0.format(n) : nf1.format(n)} ${U[i]}`;
}
/** [değer, birim], birim soluk gösterilsin diye ayrı */
export function bytesParts(n: number): [string, string] {
  const s = bytes(n);
  const k = s.lastIndexOf(" ");
  return [s.slice(0, k), s.slice(k + 1)];
}
export const speed = (n: number) => (n > 0 ? `${bytes(n)}/sn` : "");

/** kalan süre. 10 sn altı "Birkaç saniye", 1 dk altı 5 sn'ye yuvarla, 1 sa altı dakikayı yukarı yuvarla, sonrası en çok iki birim. -1 hesaplanıyor, -2 bekleniyor */
export function eta(s: number): string {
  if (s === -2) return "Bekleniyor";
  if (s < 0 || !isFinite(s)) return "";
  if (s < 10) return "Birkaç saniye";
  if (s < 60) return `${Math.max(10, Math.round(s / 5) * 5)} sn`;
  if (s < 3600) return `${Math.ceil(s / 60)} dk`;
  const h = Math.floor(s / 3600), m = Math.ceil((s % 3600) / 60);
  if (h >= 48) return `${Math.round(h / 24)} gün`;
  return m === 60 ? `${h + 1} sa` : m ? `${h} sa ${m} dk` : `${h} sa`;
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
  if (d < 60) return "Az önce";
  if (d < 3600) return `${Math.floor(d / 60)} dk önce`;
  if (d < 86400) return `${Math.floor(d / 3600)} sa önce`;
  const dt = new Date(ts * 1000);
  if (d < 86400 * 2) return `Dün ${String(dt.getHours()).padStart(2, "0")}:${String(dt.getMinutes()).padStart(2, "0")}`;
  return `${dt.getDate()} ${MONTHS[dt.getMonth()]}${dt.getFullYear() !== new Date().getFullYear() ? " " + dt.getFullYear() : ""}`;
}
export function absTime(ts: number): string {
  return ts ? new Date(ts * 1000).toLocaleString("tr-TR", { dateStyle: "long", timeStyle: "short" }) : "";
}
export function compact(n: number): string {
  return new Intl.NumberFormat("tr-TR", { notation: "compact", maximumFractionDigits: 1 }).format(n);
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
