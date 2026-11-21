import { call, type Settings, type Snapshot, type Task } from "./api";
import { t } from "./i18n.svelte";

export const CATS: Record<string, { label: string; icon: string }> = {
  video: { label: "Video", icon: "video" },
  muzik: { label: "Müzik", icon: "music" },
  belge: { label: "Belgeler", icon: "doc" },
  program: { label: "Programlar", icon: "app" },
  arsiv: { label: "Arşivler", icon: "archive" },
  resim: { label: "Resimler", icon: "image" },
  diger: { label: "Diğer", icon: "file" },
};
export const ACTIVE = new Set(["downloading", "probing", "processing"]);

type Modal =
  | { type: "add"; url?: string; headers?: Record<string, string>; filename?: string; external?: boolean }
  | { type: "grab"; data: any }
  | { type: "settings"; tab?: string }
  | { type: "ext" }
  | { type: "confirm-delete"; ids: string[] }
  | { type: "update-url"; id: string };

class Store {
  snap = $state<Snapshot>({ tasks: [], speed: 0, history: [], session: 0, eta_total: -1 });
  settings = $state<Settings | null>(null);
  filter = $state("all");
  query = $state("");
  selected = $state<Set<string>>(new Set());
  anchor = $state<string | null>(null);
  detail = $state(true);
  hidden = $state<Set<string>>(new Set()); // geri alınabilir kaldırmayı bekleyenler
  modal = $state<Modal | null>(null);
  ctx = $state<{ x: number; y: number; id: string } | null>(null);
  tools = $state<{ ytdlp: string; ytdlp_version: string; ffmpeg: string; deno?: string } | null>(null);
  justDone = $state<Set<string>>(new Set());
  /** az önce gelenler, vurgu için kısa süre tutuluyor */
  justAdded = $state<Set<string>>(new Set());
  #seen: Set<string> | null = null;

  /** her tıkta gelen anlık görüntü. yeni id'ler işaretlenir, toplu eklemede (>5) animasyon yok */
  ingest(s: Snapshot) {
    if (s.ids) {
      // ids varsa yalnızca değişen satırlar geldi
      const m = new Map(this.snap.tasks.map((t) => [t.id, t]));
      for (const t of s.tasks) m.set(t.id, t);
      s = { ...s, tasks: s.ids.map((id) => m.get(id)).filter((t): t is Task => !!t) };
    }
    const seen = this.#seen;
    if (!seen) flushPending();
    if (seen) {
      const added = s.tasks.filter((t) => !seen.has(t.id)).map((t) => t.id);
      if (added.length && added.length <= 5 && !reducedMotion()) {
        this.justAdded = new Set([...this.justAdded, ...added]);
        setTimeout(() => {
          const n = new Set(this.justAdded);
          for (const id of added) n.delete(id);
          this.justAdded = n;
        }, 900);
      }
    }
    this.#seen = new Set(s.tasks.map((t) => t.id));
    this.snap = s;
  }

  get tasks(): Task[] {
    return this.snap.tasks;
  }
  get visible(): Task[] {
    const q = this.query.trim().toLocaleLowerCase("tr");
    return this.snap.tasks.filter((t) => {
      if (this.hidden.has(t.id)) return false;
      if (q && !`${t.name} ${t.title} ${t.url}`.toLocaleLowerCase("tr").includes(q)) return false;
      return match(t, this.filter);
    });
  }
  count(f: string): number {
    let n = 0;
    for (const t of this.snap.tasks) if (!this.hidden.has(t.id) && match(t, f)) n++;
    return n;
  }
  get focused(): Task | undefined {
    if (this.selected.size !== 1) return undefined;
    const id = [...this.selected][0];
    return this.snap.tasks.find((t) => t.id === id);
  }
  select(id: string, e?: { ctrlKey?: boolean; metaKey?: boolean; shiftKey?: boolean }) {
    const v = this.visible;
    if (e?.shiftKey && this.anchor) {
      const a = v.findIndex((t) => t.id === this.anchor), b = v.findIndex((t) => t.id === id);
      if (a >= 0 && b >= 0) {
        const [lo, hi] = a < b ? [a, b] : [b, a];
        this.selected = new Set(v.slice(lo, hi + 1).map((t) => t.id));
        return;
      }
    }
    if (e?.ctrlKey || e?.metaKey) {
      const n = new Set(this.selected);
      n.has(id) ? n.delete(id) : n.add(id);
      this.selected = n;
    } else {
      this.selected = new Set([id]);
    }
    this.anchor = id;
  }
  selectAll() {
    this.selected = new Set(this.visible.map((t) => t.id));
  }
  clearSelection() {
    this.selected = new Set();
  }

  /** hemen gizle, 6 sn içinde geri alınmazsa gerçekten kaldır */
  remove(ids: string[]) {
    if (!ids.length) return;
    const h = new Set(this.hidden);
    ids.forEach((i) => h.add(i));
    this.hidden = h;
    const sel = new Set(this.selected);
    ids.forEach((i) => sel.delete(i));
    this.selected = sel;
    // gizliyken inmeye devam etmesin, geri alınırsa sürer
    const running = ids.filter((id) => { const t = this.snap.tasks.find((x) => x.id === id); return !!t && (ACTIVE.has(t.status) || t.status === "queued"); });
    running.forEach((id) => call("pause", { id }));
    pending(ids, true);
    let undone = false;
    const timer = setTimeout(() => {
      if (undone) return;
      ids.forEach((id) => call("remove", { id, deleteFile: false }));
      pending(ids, false);
      setTimeout(() => {
        const h2 = new Set(this.hidden);
        ids.forEach((i) => h2.delete(i));
        this.hidden = h2;
      }, 800);
    }, 6000);
    toasts.push({
      kind: "info",
      title: ids.length === 1 ? t("İndirme listeden kaldırıldı") : t("{n} indirme listeden kaldırıldı", { n: ids.length }),
      ttl: 6000,
      actions: [{
        label: t("Geri al"),
        primary: true,
        run: () => {
          undone = true;
          clearTimeout(timer);
          pending(ids, false);
          running.forEach((id) => call("resume", { id }));
          const h2 = new Set(this.hidden);
          ids.forEach((i) => h2.delete(i));
          this.hidden = h2;
        },
      }],
    });
  }

  async saveSettings(patch: Partial<Settings>) {
    if (!this.settings) return;
    const themeChange = "theme" in patch || "accent" in patch;
    this.settings = { ...this.settings, ...patch };
    applyLook(this.settings, themeChange);
    await call("set_settings", { s: this.settings });
  }
}

// geri al süresinde uygulama kapanırsa kaldırma bir sonraki açılışta tamamlansın
const PENDING = "trawl.pendingRemove";
function pending(ids: string[], add: boolean) {
  const s = new Set<string>(JSON.parse(localStorage.getItem(PENDING) || "[]"));
  ids.forEach((i) => (add ? s.add(i) : s.delete(i)));
  localStorage.setItem(PENDING, JSON.stringify([...s]));
}
function flushPending() {
  const ids: string[] = JSON.parse(localStorage.getItem(PENDING) || "[]");
  ids.forEach((id) => call("remove", { id, deleteFile: false }));
  localStorage.removeItem(PENDING);
}

function match(t: Task, f: string): boolean {
  switch (f) {
    case "all": return true;
    case "active": return ACTIVE.has(t.status);
    case "queued": return t.status === "queued" || t.status === "paused";
    case "done": return t.status === "done";
    case "error": return t.status === "error";
    default: return t.category === f;
  }
}

const mq = typeof matchMedia !== "undefined" ? matchMedia("(prefers-color-scheme: dark)") : null;
const rq = typeof matchMedia !== "undefined" ? matchMedia("(prefers-reduced-motion: reduce)") : null;

/** tema değişirken geçişleri bir kare kapat, yoksa her şey birden canlanıyor */
export function applyLook(s: Settings, switching = false) {
  const root = document.documentElement;
  if (switching) root.classList.add("theme-switching");
  const dark = s.theme === "dark" || (s.theme !== "light" && !!mq?.matches);
  root.dataset.theme = dark ? "dark" : "light";
  root.dataset.accent = s.accent || "cobalt";
  const mode = s.motion_mode || "system";
  const reduced = mode === "reduced" || (mode === "system" && !!rq?.matches);
  root.classList.toggle("motion-reduced", reduced);
  root.classList.toggle("motion-off", mode === "off");
  root.dataset.density = s.density || "default";
  try { localStorage.setItem("trawl.look", JSON.stringify({ dark, accent: root.dataset.accent })); } catch {}
  if (switching) requestAnimationFrame(() => requestAnimationFrame(() => root.classList.remove("theme-switching")));
}
export function watchSystem(get: () => Settings | null) {
  mq?.addEventListener("change", () => { const s = get(); if (s) applyLook(s, true); });
  rq?.addEventListener("change", () => { const s = get(); if (s) applyLook(s); });
}
export const reducedMotion = () => document.documentElement.classList.contains("motion-reduced") || document.documentElement.classList.contains("motion-off");

export const store = new Store();

// bildirimler
export interface Toast { id: number; kind: "ok" | "error" | "info" | "clip" | "warn"; title: string; body?: string; actions?: { label: string; primary?: boolean; run: () => void }[]; ttl: number }
class Toasts {
  list = $state<Toast[]>([]);
  n = 0;
  timers = new Map<number, { left: number; started: number; h: any }>();
  push(t: Omit<Toast, "id" | "ttl"> & { ttl?: number }) {
    const id = ++this.n;
    const ttl = t.ttl ?? 5000;
    const next = [...this.list, { ...t, ttl, id }];
    while (next.length > 3) { const old = next.shift()!; this.clear(old.id); }
    this.list = next;
    if (ttl > 0) this.arm(id, ttl);
    return id;
  }
  arm(id: number, left: number) {
    const h = setTimeout(() => this.close(id), left);
    this.timers.set(id, { left, started: Date.now(), h });
  }
  /** üzerine gelince süre durur, çıkınca kaldığı yerden devam */
  hold(id: number) {
    const t = this.timers.get(id);
    if (!t) return;
    clearTimeout(t.h);
    t.left = Math.max(1200, t.left - (Date.now() - t.started));
  }
  release(id: number) {
    const t = this.timers.get(id);
    if (t) this.arm(id, t.left);
  }
  clear(id: number) {
    const t = this.timers.get(id);
    if (t) clearTimeout(t.h);
    this.timers.delete(id);
  }
  close(id: number) {
    this.clear(id);
    this.list = this.list.filter((x) => x.id !== id);
  }
}
export const toasts = new Toasts();

// kısa iki notalı tık sesi
let ac: AudioContext | null = null;
export function chime() {
  if (!store.settings?.sound) return;
  try {
    ac ??= new AudioContext();
    const t0 = ac.currentTime;
    [[1046.5, 0], [1568, 0.07]].forEach(([f, d]) => {
      const o = ac!.createOscillator(), g = ac!.createGain();
      o.type = "sine"; o.frequency.value = f;
      g.gain.setValueAtTime(0, t0 + d);
      g.gain.linearRampToValueAtTime(0.05, t0 + d + 0.012);
      g.gain.exponentialRampToValueAtTime(0.0001, t0 + d + 0.25);
      o.connect(g).connect(ac!.destination);
      o.start(t0 + d); o.stop(t0 + d + 0.3);
    });
  } catch {}
}
