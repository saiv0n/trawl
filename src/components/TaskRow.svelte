<script lang="ts">
  import Icon from "./Icon.svelte";
  import Progress from "./Progress.svelte";
  import { call, type Task } from "../lib/api";
  import { store, toasts, CATS, ACTIVE } from "../lib/store.svelte";
  import { bytesParts, speed, eta, host, ago, middle, absTime } from "../lib/fmt";

  let { t, nameW = 240 }: { t: Task; nameW?: number } = $props();

  const active = $derived(ACTIVE.has(t.status));
  const sel = $derived(store.selected.has(t.id));
  const settle = $derived(store.justDone.has(t.id));
  const pct = $derived(t.size ? Math.min(1, t.done / t.size) : 0);
  const stalled = $derived(t.status === "downloading" && t.eta === -2);
  const displayName = $derived(middle(t.name || t.title || host(t.url) || "Hazırlanıyor", nameW));
  const STATUS: Record<string, [string, string]> = {
    queued: ["Sırada", "clock"], probing: ["Bağlanıyor", "spin"], downloading: ["İndiriliyor", "down"], processing: ["İşleniyor", "spin"],
    paused: ["Duraklatıldı", "pause"], done: ["Tamamlandı", "ok"], error: ["İndirilemedi", "warn"],
  };
  const st = $derived.by(() => {
    if (stalled) return ["Bekleniyor", "offline"];
    if ((t.status === "processing" || t.status === "probing") && t.stage) return [t.stage, "spin"];
    return STATUS[t.status];
  });
  const barState = $derived(
    t.status === "processing" ? "phase" : t.status === "probing" || (active && !t.size) ? "indet" : t.status === "downloading" ? "active" : t.status === "error" ? "error" : "paused",
  );
  // ikon sadece aynı indirmenin durumu değişince dönüşsün, kaydırmada satır yeniden kullanılınca değil
  // svelte-ignore state_referenced_locally
  let prevId = t.id;
  // svelte-ignore state_referenced_locally
  let prevSt = st[1];
  let flip = $state(0);
  $effect.pre(() => {
    const id = t.id, s = st[1];
    if (id === prevId && s !== prevSt) flip++;
    prevId = id;
    prevSt = s;
  });
  const arrive = $derived(store.justAdded.has(t.id));
  const size = $derived(bytesParts(t.size || t.done));
  const done = $derived(bytesParts(t.done));

  function toggle(e: MouseEvent) {
    e.stopPropagation();
    if (active) call("pause", { id: t.id });
    else if (t.status === "queued") call("start_now", { id: t.id });
    else call("resume", { id: t.id });
  }
  async function open(e?: MouseEvent) {
    e?.stopPropagation();
    try { await call("open_task", { id: t.id }); } catch (x) { toasts.push({ kind: "error", title: "Açılamadı", body: String(x) }); }
  }
  function menu(e: MouseEvent) {
    e.preventDefault();
    if (!store.selected.has(t.id)) store.select(t.id);
    store.ctx = { x: e.clientX, y: e.clientY, id: t.id };
  }
</script>

<div
  class="row s-{t.status}"
  class:sel
  class:settle
  class:arrive
  role="row"
  aria-selected={sel}
  tabindex="-1"
  onclick={(e) => store.select(t.id, e)}
  ondblclick={() => (t.status === "done" ? open() : (store.detail = true))}
  oncontextmenu={menu}
  onkeydown={() => {}}
>
  <div class="c-ic" role="gridcell">
    <Icon n={(CATS[t.category] || CATS.diger).icon} size={16} />
  </div>

  <div class="c-name" role="gridcell" title={t.name || t.title}>
    <span class="nm">{displayName}</span>
    <span class="meta ellip">
      {#if t.status === "error"}<span class="err">{t.error}</span>{:else}<span class="host">{host(t.url)}{#if t.quality}{" · "}{t.quality}{/if}</span>{/if}
      <!-- dar pencerede gizlenen sütunlar buraya iniyor, bilgi kaybolmasın -->
      {#if t.status !== "done" && t.status !== "error" && t.size}<span class="lv lv-sz num">{done[0]} {done[1]} / {size[0]} {size[1]}</span>{/if}
      {#if active && t.speed > 0}<span class="lv lv-sp num">{speed(t.speed)}{t.status === "downloading" && t.eta !== -2 ? ` · ${eta(t.eta)}` : ""}</span>{/if}
    </span>
  </div>

  <div class="c-st st-{t.status}" class:stalled role="gridcell">
    {#if settle}
      <svg class="tick" viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="7" /><path d="M4.8 8.3 7 10.4l4.2-4.6" /></svg>
    {:else}
      {#key flip}<span class="si" class:morph={flip > 0}><Icon n={st[1]} size={14} filled={t.status === "done" || t.status === "error"} cls={st[1] === "spin" ? "spinning" : ""} /></span>{/key}
    {/if}
    <span class="ellip">{st[0]}</span>
  </div>

  <div class="c-pr" role="gridcell">
    {#if t.status === "done"}
      {#if settle}<div class="collapse"><Progress pct={1} state="done" /></div>{:else}<span class="dim num" title={absTime(t.finished)}>{ago(t.finished)}</span>{/if}
    {:else}
      <Progress {pct} state={barState} notches={t.kind === "http" ? t.segs.length : 0} />
      <span class="pct num">{t.size && t.status !== "processing" ? `%${Math.floor(pct * 100)}` : ""}</span>
    {/if}
  </div>

  <div class="c-sz num" role="gridcell">
    {#if t.status === "done" || !t.size}
      {#if t.size || t.done}{size[0]}<small>{size[1]}</small>{:else}<span class="dim">—</span>{/if}
    {:else}
      {done[0]}<small>{done[1]}</small><span class="dim">{" / "}{size[0]}<small>{size[1]}</small></span>
    {/if}
  </div>
  <div class="c-sp num" role="gridcell">{active && t.speed > 0 ? speed(t.speed) : ""}</div>
  <div class="c-eta num" role="gridcell">{t.status === "downloading" && t.eta !== -2 ? eta(t.eta) : ""}</div>

  <div class="c-act" role="gridcell">
    {#if t.status === "done"}
      <button class="icon-btn pri" aria-label="Aç" data-tip="Aç" onclick={open}><Icon n="external" size={15} /></button>
    {:else if t.status !== "processing"}
      <button class="icon-btn pri" aria-label={active ? "Duraklat" : t.status === "error" ? "Yeniden dene" : "Sürdür"} data-tip={active ? "Duraklat" : t.status === "error" ? "Yeniden dene" : "Sürdür"} onclick={toggle}>
        {#key flip}<span class="si" class:morph={flip > 0}><Icon n={active ? "pause" : t.status === "error" ? "retry" : "play"} size={15} /></span>{/key}
      </button>
    {/if}
    <button class="icon-btn" aria-label="Klasörde göster" data-tip="Klasörde göster" onclick={(e) => { e.stopPropagation(); call("reveal_task", { id: t.id }); }}><Icon n="folder" size={15} /></button>
    <button class="icon-btn" aria-label="Diğer" onclick={(e) => { e.stopPropagation(); menu(e); }}><Icon n="more" size={15} /></button>
  </div>
</div>

<style>
  .row { display: grid; grid-template-columns: var(--cols); align-items: center; gap: var(--gap); height: var(--row); padding: 0 8px 0 14px; position: relative; border-bottom: 1px solid var(--border-subtle); contain: strict; transition: background-color 120ms; }
  .row:hover { background: var(--hover); transition-duration: 0ms; }
  .row.sel { background: var(--selected); }
  .row.sel::before { content: ""; position: absolute; left: 0; top: 6px; bottom: 6px; width: 2px; border-radius: 0 2px 2px 0; background: var(--accent); }
  .c-ic { color: var(--text-3); display: grid; place-items: center; }
  .s-error .c-ic { color: var(--error); }

  .c-name { min-width: 0; display: flex; flex-direction: column; justify-content: center; }
  .nm { font-weight: 500; white-space: nowrap; overflow: hidden; }
  .meta { font-size: 12px; line-height: 16px; color: var(--text-3); }
  .err { color: var(--error); }
  :global([data-density="compact"]) .meta { display: none; }

  .c-st { display: flex; align-items: center; gap: 6px; font-size: 12.5px; color: var(--text-2); min-width: 0; }
  .c-st :global(.ic) { color: var(--text-3); }
  .st-downloading { color: var(--text); }
  .st-downloading :global(.ic) { color: var(--accent); }
  .st-done :global(.ic) { color: var(--ok); }
  .st-error { color: var(--error); }
  .st-error :global(.ic) { color: var(--error); }
  .stalled :global(.ic) { color: var(--warn); }
  :global(svg.spinning) { animation: spin 0.9s linear infinite; }

  .c-pr { display: flex; align-items: center; gap: 10px; min-width: 0; }
  .pct { width: 34px; flex: none; text-align: right; font-size: 12px; color: var(--text-2); }
  .c-sz, .c-sp, .c-eta { text-align: right; font-size: 12.5px; white-space: nowrap; color: var(--text-2); overflow: hidden; }
  small { font-size: inherit; color: var(--text-3); margin-left: 2px; }
  .c-sz .dim { color: var(--text-3); }
  .c-sp { color: var(--text); }

  /* yerleri baştan ayrılmış, sadece görünürlük değişiyor. ana eylem hep görünür, gerisi üzerine gelince */
  .c-act { display: flex; justify-content: flex-end; gap: 2px; }
  .c-act > :not(.pri) { opacity: 0; transition: opacity 120ms; }
  .row:hover .c-act > *, .row.sel .c-act > *, .row:focus-within .c-act > * { opacity: 1; transition-duration: 0ms; }
  .c-act .pri { color: var(--text-2); }
  .si { display: inline-grid; place-items: center; flex: none; }
  /* ikon dönüşümü 160 ms, sadece transform ve opacity */
  .morph { animation: morph 160ms var(--ease-out); }
  @keyframes morph { from { opacity: 0; transform: scale(0.6) rotate(-25deg); } }
  /* yeni satır yukarıdan iner, vurgu 700 ms'de söner ki nereye geldiği görülsün */
  .arrive { animation: arrive-tint 800ms var(--ease-out); }
  .arrive > * { animation: arrive-in 220ms var(--ease-out) both; }
  @keyframes arrive-tint { from { background-color: color-mix(in oklch, var(--accent) 12%, transparent); } }
  @keyframes arrive-in { from { opacity: 0; transform: translateY(-8px); } }
  :global(.motion-reduced) .morph, :global(.motion-reduced) .arrive, :global(.motion-reduced) .arrive > * { animation: none; }
  .lv { display: none; color: var(--text-2); }
  .lv::before { content: " · "; color: var(--text-3); }
  @container (width < 940px) { .lv-sz { display: inline; } }
  @container (width < 820px) { .lv-sp { display: inline; } .host { display: none; } .lv-sz::before { content: ""; } }

  /* tamamlanma animasyonu */
  .tick { width: 14px; height: 14px; flex: none; fill: none; stroke: var(--ok); stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; animation: pop 430ms var(--spring-pop) both; }
  .tick circle { stroke-dasharray: 44; stroke-dashoffset: 44; animation: draw 300ms var(--ease-out) 60ms forwards; }
  .tick path { stroke-dasharray: 12; stroke-dashoffset: 12; animation: draw 250ms var(--ease-out) 260ms forwards; }
  @keyframes draw { to { stroke-dashoffset: 0; } }
  @keyframes pop { from { transform: scale(0.8); } to { transform: scale(1); } }
  .collapse { width: 100%; animation: collapse 250ms var(--ease-exit) 350ms both; transform-origin: left center; }
  @keyframes collapse { to { transform: scaleY(0); opacity: 0; } }
  .settle { animation: tint 900ms var(--ease-out); }
  @keyframes tint { from { background-color: var(--ok-soft); } }
  :global(.motion-reduced) .tick, :global(.motion-reduced) .tick circle, :global(.motion-reduced) .tick path { animation: none; stroke-dashoffset: 0; }
</style>
