<script lang="ts">
  import { onDestroy } from "svelte";
  import Icon from "./Icon.svelte";
  import { call, on } from "../lib/api";
  import { store, toasts } from "../lib/store.svelte";
  import { bytes, host } from "../lib/fmt";
  import { t } from "../lib/i18n.svelte";

  let { data, headers = {}, onclose }: { data: any; headers?: Record<string, string>; onclose: () => void; standalone?: boolean } = $props();

  type It = { url: string; kind: string; name: string; ext: string; size: number; mime: string; source: string; thumb: string; title: string; page: string; headers?: Record<string, string> };
  // svelte-ignore state_referenced_locally
  let page = $state<{ url: string; title: string; icon: string; pages: number; items: It[] }>(data);
  let tab = $state("all");
  let q = $state("");
  let minSize = $state(true);
  let picked = $state<Set<string>>(new Set());
  let depth = $state(0);
  let scanning = $state(false);
  let prog = $state({ pages: 0, found: 0 });

  const KINDS: [string, string, string][] = [
    ["all", "Tümü", "list"], ["video", "Video", "video"], ["audio", "Ses", "music"], ["stream", "Akış", "stream"], ["embed", "Gömülü", "embed"],
    ["image", "Resim", "image"], ["doc", "Belge", "doc"], ["archive", "Arşiv", "archive"], ["app", "Program", "app"], ["unknown", "Diğer", "file"],
  ];
  const ICON: Record<string, string> = Object.fromEntries(KINDS.map(([k, , i]) => [k, i]));
  // "1 MB üstü" ikonları ve küçük görselleri eler, boyutu bilinmeyen kalır
  const base = $derived(page.items.filter((i) => !(minSize && i.size > 0 && i.size < 1024 * 1024 && (i.kind === "image" || i.kind === "unknown"))));
  const count = (k: string) => (k === "all" ? base.length : base.filter((i) => i.kind === k).length);
  const list = $derived(base.filter((i) => (tab === "all" || i.kind === tab) && (!q || `${i.name} ${i.title} ${i.url}`.toLocaleLowerCase("tr").includes(q.toLocaleLowerCase("tr")))));
  const selSize = $derived(page.items.filter((i) => picked.has(i.url)).reduce((a, i) => a + (i.size || 0), 0));
  const allOn = $derived(list.length > 0 && list.every((i) => picked.has(i.url)));

  // başta sadece dosyaya benzeyenler seçili
  picked = new Set(page.items.filter((i) => !["image", "unknown"].includes(i.kind)).slice(0, 50).map((i) => i.url));

  function toggle(u: string) { const n = new Set(picked); n.has(u) ? n.delete(u) : n.add(u); picked = n; }
  function toggleAll() {
    const n = new Set(picked);
    for (const i of list) allOn ? n.delete(i.url) : n.add(i.url);
    picked = n;
  }

  let off: (() => void) | undefined;
  on("scrape-progress", (p: any) => (prog = p)).then((f) => (off = f));
  onDestroy(() => off?.());

  async function deep(d: number) {
    depth = d; scanning = true; prog = { pages: 0, found: 0 };
    try {
      const r: any = await call("scrape_page", { url: page.url, depth: d, headers });
      const had = new Set(page.items.map((i) => i.url));
      page = r;
      const n = new Set(picked);
      for (const i of r.items) if (!had.has(i.url) && !["image", "unknown"].includes(i.kind)) n.add(i.url);
      picked = n;
    } catch (e) { toasts.push({ kind: "error", title: t("Sayfa taranamadı"), body: t(String(e)) }); }
    scanning = false;
  }

  async function download() {
    const s = store.settings!;
    const opts = { mode: "video", height: s.default_height, container: s.default_container, codec: s.default_codec, bitrate: s.default_bitrate, subs: [], chapters: true, sponsorblock: false };
    // panodan gelenlerin sayfası yok, boş Referer yollama
    const hdrs = (i: It) => ({ ...(i.page || page.url ? { Referer: i.page || page.url } : {}), ...headers, ...(i.headers || {}) });
    const items = page.items.filter((i) => picked.has(i.url)).map((i) =>
      i.kind === "embed" || i.kind === "stream"
        ? { kind: "media", url: i.url, title: i.title || i.name, headers: hdrs(i), opts }
        : { kind: "http", url: i.url, name: i.name, page: i.page || page.url, headers: hdrs(i) },
    );
    if (!items.length) return;
    const n = await call("add_many", { list: items });
    toasts.push({ kind: "ok", title: t("{n} indirme sıraya eklendi", { n: n ?? items.length }), body: page.title || host(page.url) });
    onclose();
  }
</script>

<div class="gb">
  <header>
    {#if page.icon}<img class="fav" src={page.icon} alt="" referrerpolicy="no-referrer" />{:else}<Icon n="globe" size={18} />{/if}
    <div class="ht">
      <h2 class="ellip">{page.title || host(page.url)}</h2>
      <p class="num">{host(page.url)} · {t("{p} sayfa tarandı · {n} öğe", { p: page.pages, n: page.items.length })}</p>
    </div>
    <div class="seg" aria-label={t("Tarama derinliği")}>
      {#each [[0, "Bu sayfa"], [1, "1 seviye"], [2, "2 seviye"]] as [d, l]}
        <button class:on={depth === d} disabled={scanning} onclick={() => deep(d as number)}>{t(l as string)}</button>
      {/each}
    </div>
    <button class="icon-btn" aria-label={t("Kapat")} onclick={onclose}><Icon n="x" /></button>
  </header>

  <div class="tabs" role="tablist">
    {#each KINDS as [k, l, i]}
      {#if k === "all" || count(k)}
        <button role="tab" aria-selected={tab === k} class:on={tab === k} onclick={() => (tab = k)}><Icon n={i} size={14} />{t(l)}<em class="num">{count(k)}</em></button>
      {/if}
    {/each}
  </div>

  <div class="tools">
    <label class="q"><Icon n="search" size={14} /><input bind:value={q} placeholder={t("Süz")} spellcheck="false" aria-label={t("Süz")} /></label>
    <button class="toggle" onclick={() => (minSize = !minSize)}><span class="switch" class:on={minSize}></span>{t("Küçük görselleri gizle")}</button>
  </div>

  <div class="list" role="grid" aria-multiselectable="true">
    <div class="lh">
      <button class="check" class:on={allOn} role="checkbox" aria-checked={allOn} aria-label={t("Görünenlerin tümünü seç")} onclick={toggleAll}><Icon n="check" size={11} /></button>
      <span>{t("Ad")}</span><span>{t("Kaynak")}</span><span class="r">{t("Boyut")}</span>
    </div>
    <div class="lb">
      {#if scanning}
        <div class="scan" aria-live="polite"><span class="spinner"></span><b class="num">{t("{p} sayfa tarandı · {n} öğe", { p: prog.pages, n: prog.found })}</b><small>{t("Aynı sitedeki bağlantılar geziliyor")}</small></div>
      {/if}
      {#each list as i (i.url)}
        <button class="it" class:on={picked.has(i.url)} role="row" aria-selected={picked.has(i.url)} onclick={() => toggle(i.url)}>
          <span class="check" class:on={picked.has(i.url)}><Icon n="check" size={11} /></span>
          {#if i.kind === "image"}<img class="pv" src={i.url} alt="" loading="lazy" referrerpolicy="no-referrer" />{:else}<span class="ki"><Icon n={ICON[i.kind] || "file"} size={15} /></span>{/if}
          <span class="nm">
            <b class="ellip">{i.title && i.kind !== "image" ? i.title : i.name || host(i.url)}</b>
            <small class="mono ellip">{i.url}</small>
          </span>
          <span class="src">{i.ext ? i.ext.toUpperCase() : i.kind === "embed" ? t("Gömülü") : i.kind === "stream" ? t("Akış") : ""}</span>
          <span class="sz num">{i.size ? bytes(i.size) : i.kind === "embed" || i.kind === "stream" ? "Video" : "—"}</span>
        </button>
      {:else}
        {#if !scanning}
          <div class="none">
            {#if page.items.length}
              <p>{t("Süzgece uyan dosya yok.")}</p>
              <button class="btn btn-sm" onclick={() => { q = ""; tab = "all"; minSize = false; }}>{t("Süzgeçleri temizle")}</button>
            {:else}
              <p>{t("Bu sayfada indirilebilir dosya bulunamadı.")}</p>
              <button class="btn btn-sm" onclick={() => deep(1)}>{t("Bağlantılı sayfaları da tara")}</button>
            {/if}
          </div>
        {/if}
      {/each}
    </div>
  </div>

  <footer>
    <span class="sum num"><b>{picked.size}</b> {t("seçili")}{selSize ? ` · ${bytes(selSize)}` : ""}</span>
    <button class="btn btn-lg" onclick={onclose}>{t("Vazgeç")}</button>
    <button class="btn btn-primary btn-lg" disabled={!picked.size} onclick={download}><Icon n="down" size={14} />{t("{n} dosyayı indir", { n: picked.size })}</button>
  </footer>
</div>

<style>
  .gb { width: 880px; max-width: 100%; display: flex; flex-direction: column; min-height: 0; max-height: calc(100vh - 64px); }
  header { display: flex; align-items: center; gap: 12px; padding: 14px 16px 12px 20px; border-bottom: 1px solid var(--border-subtle); color: var(--text-3); }
  .fav { width: 18px; height: 18px; object-fit: contain; }
  .ht { flex: 1; min-width: 0; }
  h2 { font-size: 16px; line-height: 22px; font-weight: 600; color: var(--text); }
  .ht p { font-size: 12px; color: var(--text-3); }
  .tabs { display: flex; gap: 2px; padding: 0 16px; border-bottom: 1px solid var(--border-subtle); overflow-x: auto; flex: none; }
  .tabs button { height: 36px; display: flex; align-items: center; gap: 6px; padding: 0 10px; color: var(--text-2); font-weight: 500; border-bottom: 2px solid transparent; margin-bottom: -1px; white-space: nowrap; }
  .tabs button:hover { color: var(--text); }
  .tabs button.on { color: var(--text); border-bottom-color: var(--accent); }
  .tabs em { font-style: normal; font-size: 12px; color: var(--text-3); }
  .tools { display: flex; align-items: center; gap: 12px; padding: 12px 20px; }
  .q { flex: 1; display: flex; align-items: center; gap: 8px; height: 32px; padding: 0 10px; border-radius: var(--r-2); background: var(--bg); border: 1px solid var(--border); color: var(--text-3); }
  .q:focus-within { border-color: var(--accent); }
  .q input { flex: 1; border: 0; background: none; outline: none; color: var(--text); }
  .toggle { display: flex; align-items: center; gap: 8px; color: var(--text-2); white-space: nowrap; }
  .list { flex: 1; min-height: 260px; display: flex; flex-direction: column; margin: 0 20px 12px; border: 1px solid var(--border); border-radius: var(--r-2); overflow: hidden; }
  .lh, .it { display: grid; grid-template-columns: 16px 30px 1fr 64px 76px; align-items: center; gap: 12px; padding: 0 12px; }
  .lh { height: 30px; grid-template-columns: 16px 1fr 64px 76px; font-size: 12px; font-weight: 500; color: var(--text-3); border-bottom: 1px solid var(--border-subtle); background: var(--bg); }
  .lh span:first-of-type { padding-left: 42px; }
  .r { text-align: right; }
  .lb { flex: 1; overflow-y: auto; position: relative; max-height: 48vh; }
  .it { width: 100%; height: 44px; text-align: left; border-bottom: 1px solid var(--border-subtle); transition: background-color 100ms; }
  .it:last-child { border-bottom: 0; }
  .it:hover { background: var(--hover); }
  .it.on { background: var(--selected); }
  .ki { width: 30px; height: 30px; border-radius: var(--r-1); display: grid; place-items: center; color: var(--text-2); background: var(--surface-2); }
  .pv { width: 30px; height: 30px; border-radius: var(--r-1); object-fit: cover; background: var(--surface-2); }
  .nm { min-width: 0; display: flex; flex-direction: column; }
  .nm b { font-weight: 500; }
  .nm small { font-size: 11.5px; color: var(--text-3); }
  .src { font-size: 12px; color: var(--text-3); text-align: right; }
  .sz { text-align: right; color: var(--text-2); font-size: 12.5px; }
  .none { padding: 40px; display: flex; flex-direction: column; align-items: center; gap: 10px; color: var(--text-2); }
  .scan { position: absolute; inset: 0; z-index: 2; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px; background: color-mix(in oklch, var(--surface) 94%, transparent); }
  .scan small { color: var(--text-3); }
  footer { display: flex; align-items: center; gap: 8px; padding: 12px 16px 12px 20px; border-top: 1px solid var(--border-subtle); background: var(--bg); }
  .sum { flex: 1; color: var(--text-2); }
  .sum b { color: var(--text); font-weight: 600; }
</style>
