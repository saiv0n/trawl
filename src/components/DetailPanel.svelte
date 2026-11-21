<script lang="ts">
  import Icon from "./Icon.svelte";
  import Progress from "./Progress.svelte";
  import { call, type Task } from "../lib/api";
  import { store, toasts, ACTIVE } from "../lib/store.svelte";
  import { bytes, speed, eta, absTime, host } from "../lib/fmt";

  let { t }: { t: Task | undefined } = $props();
  const active = $derived(!!t && ACTIVE.has(t.status));
  const pct = $derived(t?.size ? Math.min(1, t.done / t.size) : 0);
  let imgOk = $state(true);
  let sum = $state("");
  let sumRes = $state<{ ok: boolean; algo: string; got: string } | null>(null);
  let sumBusy = $state(false);
  // t her tıkta yeni nesne, yalnızca başka indirme seçilince sıfırla
  const tid = $derived(t?.id);
  $effect(() => { tid; sum = ""; sumRes = null; });
  async function verify() {
    if (!t || !sum.trim()) return;
    sumBusy = true; sumRes = null;
    try { sumRes = await call("verify_hash", { id: t.id, expected: sum }); }
    catch (x) { toasts.push({ kind: "error", title: "Doğrulanamadı", body: String(x) }); }
    sumBusy = false;
  }

  function copy(s: string) {
    call("write_clipboard", { text: s });
    toasts.push({ kind: "ok", title: "Panoya kopyalandı", ttl: 1500 });
  }
  async function open() {
    try { await call("open_task", { id: t!.id }); } catch (x) { toasts.push({ kind: "error", title: "Açılamadı", body: String(x) }); }
  }
</script>

<section class="panel" aria-label="Ayrıntılar">
  {#if !t}
    <div class="none">
      {#if store.selected.size > 1}
        <b class="num">{store.selected.size} indirme seçildi</b>
        <p>Toplu işlemler üstteki çubukta.</p>
      {:else}
        <b>Bir indirme seçin</b>
        <p>Ayrıntılar, parçalar ve bağlantı burada görünür.</p>
      {/if}
    </div>
  {:else}
    <div class="scroll">
      {#if t.thumb && imgOk}
        <img class="thumb" src={t.thumb} alt="" referrerpolicy="no-referrer" onerror={() => (imgOk = false)} />
      {/if}
      <h2 class="selectable">{t.name || t.title || host(t.url)}</h2>

      {#if t.status === "error"}
        <div class="alert" role="alert"><Icon n="warn" size={16} filled /><p class="selectable">{t.error}</p></div>
      {/if}

      {#if t.status !== "done"}
        <div class="prog">
          <div class="pline num">
            <b>{t.size ? `%${(pct * 100).toLocaleString("tr-TR", { maximumFractionDigits: 1 })}` : bytes(t.done)}</b>
            <span>{active && t.speed > 0 ? speed(t.speed) : ""}{active && t.eta >= 0 ? ` · ${eta(t.eta)} kaldı` : active && t.eta === -2 ? " · Bekleniyor" : ""}</span>
          </div>
          <Progress {pct} segs={t.segs} height={8} state={t.status === "processing" ? "phase" : t.status === "probing" || (active && !t.size) ? "indet" : t.status === "downloading" ? "active" : t.status === "error" ? "error" : "paused"} />
          {#if t.status === "processing" && t.stage}<p class="phase"><span class="spinner"></span>{t.stage}</p>{/if}
        </div>
      {/if}

      <dl>
        <dt>Boyut</dt><dd class="num">{t.size ? bytes(t.size) : "Bilinmiyor"}</dd>
        {#if t.status !== "done"}<dt>İnen</dt><dd class="num">{bytes(t.done)}</dd>{/if}
        {#if t.kind === "http" && t.segs.length}<dt>Bağlantı</dt><dd class="num">{t.conns} açık · {t.segs.length} parça</dd>{/if}
        <dt>Devam</dt><dd>{t.kind === "media" || t.resumable ? "Kaldığı yerden sürer" : "Desteklenmiyor, baştan iner"}</dd>
        {#if t.quality}<dt>Biçim</dt><dd>{t.quality}</dd>{/if}
        <dt>Eklendi</dt><dd class="num">{absTime(t.created)}</dd>
        {#if t.finished}<dt>Bitti</dt><dd class="num">{absTime(t.finished)}</dd>{/if}
      </dl>

      <div class="field-row">
        <span class="label">Bağlantı</span>
        <div class="copyline"><span class="mono ellip selectable">{t.url}</span><button class="icon-btn" aria-label="Bağlantıyı kopyala" data-tip="Kopyala" onclick={() => copy(t.url)}><Icon n="copy" size={14} /></button></div>
      </div>
      {#if t.page && t.page !== t.url}
        <div class="field-row">
          <span class="label">Kaynak sayfa</span>
          <div class="copyline"><span class="mono ellip selectable">{t.page}</span><button class="icon-btn" aria-label="Sayfayı aç" data-tip="Tarayıcıda aç" onclick={() => import("@tauri-apps/plugin-opener").then((m) => m.openUrl(t.page)).catch(() => copy(t.page))}><Icon n="external" size={14} /></button></div>
        </div>
      {/if}
      <div class="field-row">
        <span class="label">Konum</span>
        <div class="copyline"><span class="mono ellip selectable">{t.path || t.folder}</span><button class="icon-btn" aria-label="Klasörde göster" data-tip="Klasörde göster" onclick={() => call("reveal_task", { id: t.id })}><Icon n="folder" size={14} /></button></div>
      </div>
    </div>

    {#if t.status === "done" && t.path}
      <div class="sum">
        <b>Sağlama</b>
        <div class="row">
          <input class="mono" spellcheck="false" placeholder="MD5, SHA-1 ya da SHA-256 yapıştırın" bind:value={sum} onkeydown={(e) => e.key === "Enter" && verify()} aria-label="Beklenen sağlama değeri" />
          <button class="btn btn-sm" disabled={sumBusy || !sum.trim()} onclick={verify}>{#if sumBusy}<span class="spinner"></span>{/if}Doğrula</button>
        </div>
        {#if sumRes}<p class="res" class:bad={!sumRes.ok}>{sumRes.ok ? `${sumRes.algo} tutuyor, dosya sağlam` : `${sumRes.algo} tutmuyor. Dosya bozuk ya da değiştirilmiş olabilir`}</p>{/if}
      </div>
    {/if}
    <div class="acts">
      {#if t.status === "done"}
        <button class="btn btn-primary" onclick={open}><Icon n="external" size={14} />Aç</button>
      {:else if active && t.status !== "processing"}
        <button class="btn" onclick={() => call("pause", { id: t.id })}><Icon n="pause" size={14} />Duraklat</button>
      {:else if t.status !== "processing"}
        <button class="btn btn-primary" onclick={() => call("resume", { id: t.id })}><Icon n={t.status === "error" ? "retry" : "play"} size={14} />{t.status === "error" ? "Yeniden dene" : "Sürdür"}</button>
      {/if}
      {#if !active && t.status !== "done"}
        <button class="btn" onclick={() => (store.modal = { type: "update-url", id: t.id })}><Icon n="link" size={14} />Bağlantıyı güncelle</button>
      {/if}
      <div class="more">
        {#if !active}<button class="btn btn-ghost btn-sm" onclick={() => call("restart", { id: t.id })}><Icon n="retry" size={14} />Baştan indir</button>{/if}
        <button class="btn btn-ghost btn-sm" onclick={() => store.remove([t.id])}><Icon n="x" size={14} />Listeden kaldır</button>
        {#if t.path}<button class="btn btn-ghost btn-sm btn-danger" onclick={() => (store.modal = { type: "confirm-delete", ids: [t.id] })}><Icon n="trash" size={14} />Diskten sil</button>{/if}
      </div>
    </div>
  {/if}
</section>

<style>
  .sum { padding: 12px 16px; border-top: 1px solid var(--border); display: grid; gap: 6px; }
  .sum b { font-size: 12px; color: var(--text-2); font-weight: 500; }
  .sum .row { display: flex; gap: 6px; }
  .sum input { flex: 1; min-width: 0; height: 28px; padding: 0 8px; border-radius: var(--r-2); border: 1px solid var(--border); background: var(--bg); font-size: 11.5px; outline: none; }
  .sum input:focus { border-color: var(--accent); }
  .res { font-size: 12px; color: var(--ok); }
  .res.bad { color: var(--error); }
  .panel { width: 340px; flex: none; display: flex; flex-direction: column; min-height: 0; border-left: 1px solid var(--border-subtle); background: var(--bg); }
  .none { margin: auto; text-align: center; padding: 24px; color: var(--text-3); }
  .none b { display: block; color: var(--text-2); font-weight: 500; margin-bottom: 4px; }
  .scroll { flex: 1; overflow-y: auto; padding: 16px; display: flex; flex-direction: column; gap: 16px; }
  .thumb { width: 100%; aspect-ratio: 16 / 9; object-fit: cover; border-radius: var(--r-2); background: var(--surface-2); }
  h2 { font-size: 15px; line-height: 20px; font-weight: 600; word-break: break-word; }
  .alert { display: flex; gap: 8px; align-items: flex-start; padding: 10px 12px; border-radius: var(--r-2); background: var(--error-soft); color: var(--error); }
  .alert p { color: var(--text); font-size: 12.5px; }
  .prog { display: flex; flex-direction: column; gap: 8px; }
  .pline { display: flex; align-items: baseline; justify-content: space-between; gap: 8px; }
  .pline b { font-size: 20px; line-height: 26px; font-weight: 600; }
  .pline span { color: var(--text-2); font-size: 12.5px; }
  .phase { display: flex; align-items: center; gap: 8px; font-size: 12.5px; color: var(--text-2); }
  dl { display: grid; grid-template-columns: 80px 1fr; gap: 6px 12px; font-size: 12.5px; }
  dt { color: var(--text-3); }
  dd { color: var(--text); min-width: 0; }
  .field-row { display: flex; flex-direction: column; gap: 4px; }
  .copyline { display: flex; align-items: center; gap: 4px; padding-left: 10px; height: 32px; border-radius: var(--r-2); background: var(--surface-2); border: 1px solid var(--border-subtle); }
  .copyline .mono { flex: 1; color: var(--text-2); }
  .copyline .icon-btn { margin-right: 2px; }
  .acts { flex: none; display: flex; flex-wrap: wrap; gap: 6px; padding: 12px 16px; border-top: 1px solid var(--border-subtle); }
  .acts > .btn { flex: 1; }
  .more { width: 100%; display: flex; flex-wrap: wrap; gap: 2px; margin: 0 -6px; }
</style>
