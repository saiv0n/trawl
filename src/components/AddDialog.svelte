<script lang="ts">
  import { onMount, onDestroy, tick } from "svelte";
  import Icon from "./Icon.svelte";
  import Grabber from "./Grabber.svelte";
  import { call, inTauri } from "../lib/api";
  import { store, toasts, CATS } from "../lib/store.svelte";
  import { bytes, dur, compact, host, qlabel, ago, expandBatch } from "../lib/fmt";
  import { t } from "../lib/i18n.svelte";

  let { url = "", headers = {}, filename = "", external = false, onclose }: { url?: string; headers?: Record<string, string>; filename?: string; external?: boolean; onclose: () => void } = $props();
  let added = false;
  let goBtn = $state<HTMLButtonElement>();

  // svelte-ignore state_referenced_locally
  let input = $state(url);
  // svelte-ignore state_referenced_locally
  let phase = $state<"input" | "loading" | "media" | "playlist" | "http" | "page" | "batch" | "error">(url ? "loading" : "input");
  let res = $state<any>(null);
  let err = $state("");
  let toolsErr = $state(false);
  let installing = $state(false);
  let inputEl = $state<HTMLInputElement>();
  let step = $state(0);
  const STEPS = ["Bağlantı açılıyor", "Biçimler okunuyor", "Sayfa taranıyor", "Boyutlar yoklanıyor"];

  const s = store.settings!;
  let mode = $state<"video" | "audio">("video");
  let height = $state(0);
  let container = $state(s?.default_container || "mp4");
  let codec = $state(s?.default_codec || "mp3");
  let bitrate = $state(s?.default_bitrate || 320);
  let subs = $state<string[]>([]);
  let chapters = $state(true);
  let sponsor = $state(false);
  let more = $state(false);
  let adv = $state(false);
  let folder = $state("");
  let name = $state("");
  let conns = $state(s?.connections || 8);
  let picked = $state<Set<number>>(new Set());
  let batch = $state<string[]>([]);

  const AUD = [
    { c: "mp3", d: "Her yerde çalar" },
    { c: "m4a", d: "AAC, Apple" },
    { c: "opus", d: "En küçük dosya" },
    { c: "flac", d: "Kayıpsız" },
  ];
  const lossless = $derived(codec === "flac" || codec === "wav");
  const m = $derived(res?.media);
  const pageCount = $derived(res?.page?.items?.length || 0);
  const TITLES: Record<string, string> = { input: "Bağlantı ekle", loading: "Çözümleniyor", media: "İndirme seçenekleri", playlist: "Oynatma listesi", http: "Dosya", page: "Sayfadan topla", batch: "Toplu ekle", error: "Çözümlenemedi" };
  // aynı bağlantı listede zaten var mı
  const dup = $derived.by(() => {
    const u = (m?.url || res?.http?.url || input).trim();
    return u ? store.tasks.find((t) => t.url === u || t.page === u) : undefined;
  });

  // cümle ortasında küçük harf, ama ingilizce ay adı büyük kalsın
  const agoLow = (ts: number) => { const x = ago(ts); return /^[A-Z][a-z]{2} \d/.test(x) ? x : x.toLocaleLowerCase(store.settings?.lang === "en" ? "en" : "tr"); };
  async function analyze(u = input) {
    u = u.trim();
    if (!u) return;
    input = u;
    const list = expandBatch(u);
    if (list.length > 1) { batch = list; phase = "batch"; return; }
    phase = "loading"; err = ""; toolsErr = false; step = 0;
    const iv = setInterval(() => (step = Math.min(STEPS.length - 1, step + 1)), 900);
    try {
      res = await call("analyze", { url: u, headers });
      if (res.kind === "media" || res.kind === "playlist") {
        const vids = res.media.videos || [];
        mode = res.media.has_video || res.kind === "playlist" ? "video" : "audio";
        const want = s?.default_height || 1080;
        height = vids.find((v: any) => v.height <= want)?.height ?? vids[0]?.height ?? want;
        if (res.kind === "playlist") picked = new Set(res.media.entries.map((_: any, i: number) => i));
      }
      // tarayıcının bildiği ad sunucunun "download.php"sinden iyidir
      if (res.kind === "http") name = filename || res.http.name;
      phase = res.kind;
    } catch (e: any) {
      const msg = String(e);
      toolsErr = msg.startsWith("TOOLS:");
      err = msg.replace(/^TOOLS:/, "");
      phase = "error";
    } finally { clearInterval(iv); }
  }

  async function install() {
    installing = true;
    try { await call("tools_install", { which: "all" }); store.tools = await call("tools_status"); installing = false; analyze(); }
    catch (e) { installing = false; err = String(e); }
  }
  async function pickFolder() {
    if (!inTauri) return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const r = await open({ directory: true, defaultPath: folder || s.folder });
    if (typeof r === "string") folder = r;
  }
  const opts = () => ({ mode, height: mode === "video" ? height : 0, container, codec, bitrate: lossless ? 0 : bitrate, subs, chapters, sponsorblock: sponsor });

  async function go(start = true) {
    added = true;
    if (phase === "media") {
      await call("add", { p: { kind: "media", url: m.url, title: m.title, thumb: m.thumb, folder, headers, opts: opts(), start } });
    } else if (phase === "playlist") {
      const entries = m.entries.filter((_: any, i: number) => picked.has(i));
      if (!entries.length) return;
      await call("add", { p: { kind: "media", url: m.url, folder, headers, opts: opts(), start, entries } });
      toasts.push({ kind: "ok", title: t("{n} video sıraya eklendi", { n: entries.length }) });
    } else if (phase === "http") {
      await call("add", { p: { kind: "http", url: res.http.url, name, folder, headers, conns, start } });
    } else if (phase === "batch") {
      const n = await call("add_many", { list: batch.map((url) => ({ kind: "http", url, folder, headers, conns, start })) });
      toasts.push({ kind: "ok", title: t("{n} indirme sıraya eklendi", { n: n ?? batch.length }) });
    }
    onclose();
  }

  const estimate = $derived.by(() => {
    if (!m) return 0;
    if (mode === "video") return m.videos?.find((v: any) => v.height === height)?.size || 0;
    if (lossless) return m.duration ? m.duration * 110000 : 0;
    return m.duration ? (m.duration * bitrate * 1000) / 8 : 0;
  });
  const QUICK = $derived.by(() => {
    if (!m?.videos?.length) return [];
    const hs = m.videos.map((v: any) => v.height);
    const best = m.videos[0];
    const pick = (h: number) => m.videos.find((v: any) => v.height <= h);
    const out = [{ ...best, label: "En iyi" }];
    for (const h of [1080, 720]) { const v = pick(h); if (v && v.height !== best.height && !out.some((o: any) => o.height === v.height)) out.push({ ...v, label: `${v.height}p` }); }
    return hs.length ? out : [];
  });
  const defFolder = $derived(folder || (s.categorize ? `${s.folder}\\${phase === "http" ? (CATS[res?.category] || CATS.diger).label : mode === "audio" ? "Müzik" : "Video"}` : s.folder));

  async function openGrabber() {
    phase = "loading"; step = 2;
    try { res = { kind: "page", page: await call("scrape_page", { url: input, depth: 0, headers }) }; phase = "page"; }
    catch (e) { err = String(e); phase = "error"; }
  }

  // tarayıcı kendi indirmesini çoktan sildi; pencere kapanırsa indirme kaybolmasın
  onDestroy(() => {
    if (external && !added && url) {
      call("add", { p: { kind: "http", url, name: filename || name || "", headers, start: false } });
      toasts.push({ kind: "ok", title: t("Listeye duraklatılmış olarak eklendi"), body: filename || host(url) });
    }
  });
  // sonuç gelince Enter doğrudan indirsin
  $effect(() => {
    if (["http", "media", "playlist", "batch"].includes(phase)) tick().then(() => goBtn?.focus());
  });
  function keydown(e: KeyboardEvent) {
    if (e.key !== "Enter" || e.shiftKey || e.isComposing) return;
    const el = e.target as HTMLElement;
    if (el.tagName === "TEXTAREA" || (el.tagName === "BUTTON" && el !== goBtn)) return;
    if (["http", "media", "playlist", "batch"].includes(phase)) { e.preventDefault(); go(true); }
  }

  onMount(async () => {
    if (url) analyze(url);
    else {
      await tick();
      inputEl?.focus();
      const c: string = await call("read_clipboard").catch(() => "");
      if (c && /^https?:\/\//.test(c.trim()) && !input) { input = c.trim(); await tick(); inputEl?.select(); }
    }
  });
</script>

{#if phase === "page"}
  <Grabber data={res.page} {headers} {onclose} standalone />
{:else}
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="dlg" onkeydown={keydown}>
  <header>
    <div class="ht">
      <h2>{t(TITLES[phase])}</h2>
      {#if phase !== "input"}<p class="mono ellip">{host(input) || input}</p>{/if}
    </div>
    <button class="icon-btn" aria-label={t("Kapat")} onclick={onclose}><Icon n="x" /></button>
  </header>

  {#if phase === "input" || phase === "error"}
    <form class="body" onsubmit={(e) => { e.preventDefault(); analyze(); }}>
      <label class="url">
        <input bind:this={inputEl} bind:value={input} class="field mono" placeholder="https://" spellcheck="false" aria-label={t("Bağlantı")} />
      </label>
      {#if phase === "error"}
        <div class="alert" role="alert">
          <Icon n="warn" size={16} filled />
          <div>
            <b>{t(toolsErr ? "Video siteleri için bileşenler eksik" : "Bu bağlantı çözümlenemedi")}</b>
            <p>{t(toolsErr ? "yt-dlp, ffmpeg ve deno gerekiyor (yaklaşık 150 MB). Uygulama klasörüne kurulur, sistemine dokunmaz." : err)}</p>
          </div>
          {#if toolsErr}<button type="button" class="btn btn-sm" onclick={install} disabled={installing}>{#if installing}<span class="spinner"></span>{t("Kuruluyor")}{:else}{t("Şimdi kur")}{/if}</button>{/if}
        </div>
      {/if}
      <ul class="hints">
        <li><Icon n="video" size={14} />{t("Video siteleri: YouTube, Vimeo, X, Twitch ve 1800'den fazlası")}</li>
        <li><Icon n="list" size={14} />{t("Oynatma listeleri: videoları tek tek seçersiniz")}</li>
        <li><Icon n="radar" size={14} />{t("Sıradan bir sayfa: içindeki dosyaların hepsini bulur")}</li>
        <li><Icon n="down" size={14} />{t("Toplu:")} <span class="mono">{t("resim[001-120].jpg")}</span> {t("gibi aralık ya da art arda birden çok adres")}</li>
      </ul>
      <footer><span></span><button class="btn btn-primary btn-lg" type="submit" disabled={!input.trim()}>{t("Çözümle")}</button></footer>
    </form>
  {:else if phase === "loading"}
    <div class="body loading" aria-live="polite">
      <ol>
        {#each STEPS as l, i}
          <li class:done={i < step} class:cur={i === step}>
            {#if i < step}<Icon n="check" size={14} />{:else if i === step}<span class="spinner"></span>{:else}<i></i>{/if}{t(l)}
          </li>
        {/each}
      </ol>
    </div>
  {:else if phase === "media" || phase === "playlist"}
    <div class="body scroll">
      <div class="meta">
        {#if m.thumb}<img src={m.thumb} alt="" referrerpolicy="no-referrer" />{/if}
        <div class="mi">
          <h3>{m.title}</h3>
          <p class="num">{[m.uploader, m.duration ? dur(m.duration) : "", phase === "playlist" ? t("{n} video", { n: m.entries.length }) : "", m.views ? t("{n} izlenme", { n: compact(m.views) }) : ""].filter(Boolean).join(" · ")}</p>
          {#if pageCount > 1 && phase === "media"}<button class="link" onclick={openGrabber}>{t("Sayfada {n} öğe daha var, hepsini listele", { n: pageCount })}</button>{/if}
        </div>
      </div>

      {#if dup}
        <div class="note"><Icon n="info" size={15} /><span>{dup.status === "done" ? t("Bu bağlantı listede var, {when} indirildi.", { when: agoLow(dup.finished) }) : t("Bu bağlantı listede var.")}</span>{#if dup.status === "done"}<button class="btn btn-sm" onclick={() => { call("open_task", { id: dup.id }); onclose(); }}>{t("Var olanı aç")}</button>{/if}</div>
      {/if}

      <div class="seg" role="tablist">
        <button role="tab" aria-selected={mode === "video"} class:on={mode === "video"} disabled={phase === "media" && !m.has_video} onclick={() => (mode = "video")}><Icon n="video" size={14} />Video</button>
        <button role="tab" aria-selected={mode === "audio"} class:on={mode === "audio"} onclick={() => (mode = "audio")}><Icon n="music" size={14} />{t("Yalnızca ses")}</button>
      </div>

      {#if mode === "video"}
        <section>
          <span class="label">{t("Kalite")}</span>
          <div class="qlist" role="radiogroup" aria-label={t("Kalite")}>
            {#each phase === "playlist" ? [2160, 1440, 1080, 720, 480].map((h) => ({ height: h, size: 0, fps: 0, hdr: false })) : more || QUICK.length < 2 ? m.videos : QUICK as v (v.height + (v.label || ""))}
              <button class="q" role="radio" aria-checked={height === v.height} class:on={height === v.height} onclick={() => (height = v.height)}>
                <i class="radio"></i>
                <b class="num">{v.label === "En iyi" ? t("En iyi") : `${v.height}p`}</b>
                <span class="dim">{v.label === "En iyi" ? `${v.height}p · ${qlabel(v.height)}` : qlabel(v.height)}{v.fps > 30 ? ` · ${v.fps} fps` : ""}{v.hdr ? " · HDR" : ""}</span>
                <span class="sz num">{v.size ? `~${bytes(v.size)}` : phase === "playlist" ? t("en fazla") : ""}</span>
              </button>
            {/each}
          </div>
          {#if phase === "media" && QUICK.length >= 2 && m.videos.length > QUICK.length}
            <button class="link" onclick={() => (more = !more)}>{more ? t("Daha az seçenek") : t("Bütün kaliteler ({n})", { n: m.videos.length })}</button>
          {/if}
          {#if height > 1080 && container === "mp4"}<p class="hint"><Icon n="info" size={13} />{t("{h}p genelde yalnızca VP9/AV1 olarak gelir; bazı eski oynatıcılar açamayabilir. Garanti uyumluluk için 1080p seçin.", { h: height })}</p>{/if}
        </section>
        <details bind:open={adv}>
          <summary>{t("Ayrıntılı seçenekler")}</summary>
          <div class="adv">
            <div class="arow"><span class="label">{t("Kapsayıcı")}</span>
              <div class="seg">{#each [["mp4", "MP4"], ["mkv", "MKV"], ["webm", "WebM"]] as [c, l]}<button class:on={container === c} onclick={() => (container = c)}>{l}</button>{/each}</div>
            </div>
            <button class="toggle" onclick={() => (chapters = !chapters)}><span class="switch" class:on={chapters}></span>{t("Bölümleri göm")}</button>
            <button class="toggle" onclick={() => (sponsor = !sponsor)}><span class="switch" class:on={sponsor}></span>{t("Sponsor ve tanıtım kısımlarını kes")}</button>
            {#if m.subs?.length}
              <div class="arow col"><span class="label">{t("Altyazı göm")}</span>
                <div class="langs">{#each m.subs as l}<button class="badge" class:accent={subs.includes(l)} aria-pressed={subs.includes(l)} onclick={() => (subs = subs.includes(l) ? subs.filter((x) => x !== l) : [...subs, l])}>{l}</button>{/each}</div>
              </div>
            {/if}
          </div>
        </details>
      {:else}
        <section>
          <span class="label">{t("Biçim")}</span>
          <div class="qlist" role="radiogroup" aria-label={t("Ses biçimi")}>
            {#each AUD as a}
              <button class="q" role="radio" aria-checked={codec === a.c} class:on={codec === a.c} onclick={() => (codec = a.c)}>
                <i class="radio"></i><b>{a.c.toUpperCase()}</b><span class="dim">{t(a.d)}</span><span></span>
              </button>
            {/each}
          </div>
        </section>
        {#if !lossless}
          <section>
            <span class="label">{t("Bit hızı")}</span>
            <div class="seg">{#each [320, 256, 192, 128] as b}<button class="num" class:on={bitrate === b} onclick={() => (bitrate = b)}>{b} kbps</button>{/each}</div>
          </section>
        {/if}
        <button class="toggle" onclick={() => (sponsor = !sponsor)}><span class="switch" class:on={sponsor}></span>{t("Sponsor ve tanıtım kısımlarını kes")}</button>
      {/if}

      {#if phase === "playlist"}
        <section>
          <div class="lrow"><span class="label num">{t("Videolar · {a}/{b} seçili", { a: picked.size, b: m.entries.length })}</span>
            <button class="link" onclick={() => (picked = picked.size === m.entries.length ? new Set() : new Set(m.entries.map((_: any, i: number) => i)))}>{t(picked.size === m.entries.length ? "Seçimi kaldır" : "Tümünü seç")}</button>
          </div>
          <div class="plist">
            {#each m.entries as e, i}
              <button class="pe" class:on={picked.has(i)} role="checkbox" aria-checked={picked.has(i)} onclick={() => { const n = new Set(picked); n.has(i) ? n.delete(i) : n.add(i); picked = n; }}>
                <span class="check" class:on={picked.has(i)}><Icon n="check" size={11} /></span>
                <span class="num dim pi">{i + 1}</span>
                <span class="ellip pt">{e.title}</span>
                <span class="num dim">{dur(e.duration)}</span>
              </button>
            {/each}
          </div>
        </section>
      {/if}
    </div>
    <footer class="bar">
      <button class="folder" onclick={pickFolder} data-tip={t("Klasörü değiştir")}><Icon n="folder" size={14} /><span class="mono ellip">{defFolder}</span></button>
      {#if estimate && phase === "media"}<span class="num est">~{bytes(estimate)}</span>{/if}
      <button class="btn btn-lg" onclick={() => go(false)}>{t("Sıraya ekle")}</button>
      <button class="btn btn-primary btn-lg" onclick={() => go(true)} disabled={phase === "playlist" && !picked.size}><Icon n="down" size={14} />{phase === "playlist" ? t("{n} videoyu indir", { n: picked.size }) : t("İndir")}</button>
    </footer>
  {:else if phase === "batch"}
    <div class="body">
      <p class="num">{t(batch.length >= 5000 ? "{n} dosya (en fazla 5000)" : "{n} dosya", { n: batch.length })}</p>
      <ul class="blist mono">
        {#each batch.slice(0, 4) as u}<li class="ellip">{u}</li>{/each}
        {#if batch.length > 5}<li class="more num">{t("… {n} tane daha", { n: batch.length - 5 })}</li>{/if}
        {#if batch.length > 4}<li class="ellip">{batch[batch.length - 1]}</li>{/if}
      </ul>
      <section>
        <div class="lrow"><span class="label">{t("Bağlantı sayısı")}</span><b class="num">{conns}</b></div>
        <input type="range" min="1" max="32" bind:value={conns} aria-label={t("Bağlantı sayısı")} />
      </section>
    </div>
    <footer class="bar">
      <button class="folder" onclick={pickFolder} data-tip={t("Klasörü değiştir")}><Icon n="folder" size={14} /><span class="mono ellip">{folder || (s.categorize ? t("Türüne göre klasörler") : s.folder)}</span></button>
      <button class="btn btn-lg" onclick={() => go(false)}>{t("Sıraya ekle")}</button>
      <button class="btn btn-primary btn-lg" bind:this={goBtn} onclick={() => go(true)}><Icon n="down" size={14} />{t("İndir")}</button>
    </footer>
  {:else if phase === "http"}
    {@const c = CATS[res.category] || CATS.diger}
    <div class="body">
      {#if dup}
        <div class="note"><Icon n="info" size={15} /><span>{dup.status === "done" ? t("Bu dosya listede var, {when} indirildi.", { when: agoLow(dup.finished) }) : t("Bu dosya listede var.")}</span>{#if dup.status === "done"}<button class="btn btn-sm" onclick={() => { call("open_task", { id: dup.id }); onclose(); }}>{t("Var olanı aç")}</button>{/if}</div>
      {/if}
      <div class="file">
        <span class="fic"><Icon n={c.icon} size={22} /></span>
        <div class="fi">
          <label class="label" for="fname">{t("Dosya adı")}</label>
          <input id="fname" class="field" bind:value={name} spellcheck="false" />
        </div>
      </div>
      <dl>
        <dt>{t("Boyut")}</dt><dd class="num">{res.http.size ? bytes(res.http.size) : t("Bilinmiyor")}</dd>
        <dt>{t("Tür")}</dt><dd>{t(c.label)}{res.http.ctype ? ` · ${res.http.ctype}` : ""}</dd>
        <dt>{t("Devam")}</dt><dd class:ok={res.http.resumable}>{t(res.http.resumable ? "Kaldığı yerden sürer" : "Desteklemiyor, kesilirse baştan iner")}</dd>
        <dt>{t("Kaynak")}</dt><dd class="mono ellip">{res.http.url}</dd>
      </dl>
      {#if res.http.resumable}
        <section>
          <div class="lrow"><span class="label">{t("Bağlantı sayısı")}</span><b class="num">{conns}</b></div>
          <input type="range" min="1" max="32" bind:value={conns} aria-label={t("Bağlantı sayısı")} />
          <p class="hint">{t("Çoğu sunucu için 8 yeterli. Fazlası bazı sunucularda engellenmeye yol açar.")}</p>
        </section>
      {/if}
    </div>
    <footer class="bar">
      <button class="folder" onclick={pickFolder} data-tip={t("Klasörü değiştir")}><Icon n="folder" size={14} /><span class="mono ellip">{defFolder}</span></button>
      <button class="btn btn-lg" onclick={() => go(false)}>{t("Sıraya ekle")}</button>
      <button class="btn btn-primary btn-lg" bind:this={goBtn} onclick={() => go(true)}><Icon n="down" size={14} />{t("İndir")}</button>
    </footer>
  {/if}
</div>
{/if}

<style>
  .dlg { width: 600px; max-width: 100%; display: flex; flex-direction: column; min-height: 0; max-height: calc(100vh - 64px); }
  header { display: flex; align-items: flex-start; gap: 12px; padding: 16px 16px 12px 20px; border-bottom: 1px solid var(--border-subtle); }
  .ht { flex: 1; min-width: 0; }
  h2 { font-size: 16px; line-height: 22px; font-weight: 600; }
  header p { font-size: 12px; color: var(--text-3); }
  .body { padding: 16px 20px; display: flex; flex-direction: column; gap: 16px; min-height: 0; }
  .scroll { overflow-y: auto; }
  section { display: flex; flex-direction: column; gap: 8px; }
  .url .field { height: 40px; font-size: 13px; }
  .hints { list-style: none; display: flex; flex-direction: column; gap: 6px; color: var(--text-2); font-size: 12.5px; }
  .hints li { display: flex; align-items: center; gap: 8px; }
  .blist { list-style: none; display: flex; flex-direction: column; gap: 4px; font-size: 12px; color: var(--text-2); }
  .blist .more { color: var(--text-3); font-family: inherit; }
  .hints :global(.ic) { color: var(--text-3); }
  .alert { display: flex; gap: 10px; align-items: flex-start; padding: 10px 12px; border-radius: var(--r-2); background: var(--error-soft); color: var(--error); }
  .alert div { flex: 1; }
  .alert b { color: var(--text); font-weight: 600; }
  .alert p { color: var(--text-2); font-size: 12.5px; }
  .note { display: flex; align-items: center; gap: 8px; padding: 8px 8px 8px 12px; border-radius: var(--r-2); background: var(--accent-soft); color: var(--accent-text); font-size: 12.5px; }
  .note span { flex: 1; color: var(--text); }
  footer { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  footer.bar { padding: 12px 16px 12px 12px; border-top: 1px solid var(--border-subtle); background: var(--bg); }
  .folder { flex: 1; min-width: 0; height: 32px; display: flex; align-items: center; gap: 8px; padding: 0 8px; border-radius: var(--r-1); color: var(--text-2); text-align: left; }
  .folder:hover { background: var(--hover); color: var(--text); }
  .est { color: var(--text-2); padding: 0 4px; }

  .loading { min-height: 180px; justify-content: center; }
  .loading ol { list-style: none; display: flex; flex-direction: column; gap: 10px; }
  .loading li { display: flex; align-items: center; gap: 10px; color: var(--text-3); }
  .loading li.cur { color: var(--text); }
  .loading li.done { color: var(--text-2); }
  .loading li.done :global(.ic) { color: var(--ok); }
  .loading li i { width: 14px; height: 14px; border-radius: 50%; border: 1.5px solid var(--border); }

  .meta { display: flex; gap: 14px; }
  .meta img { width: 168px; aspect-ratio: 16/9; object-fit: cover; border-radius: var(--r-2); background: var(--surface-2); flex: none; }
  .mi { min-width: 0; display: flex; flex-direction: column; gap: 6px; }
  h3 { font-size: 15px; line-height: 20px; font-weight: 600; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  .mi p { color: var(--text-2); font-size: 12.5px; }
  .link { align-self: flex-start; color: var(--accent-text); font-weight: 500; font-size: 12.5px; }
  .link:hover { text-decoration: underline; }

  .qlist { border: 1px solid var(--border); border-radius: var(--r-2); overflow: hidden; }
  .q { width: 100%; display: grid; grid-template-columns: 16px 72px 1fr auto; align-items: center; gap: 10px; height: 36px; padding: 0 12px; text-align: left; border-bottom: 1px solid var(--border-subtle); transition: background-color 100ms; }
  .q:last-child { border-bottom: 0; }
  .q:hover { background: var(--hover); }
  .q.on { background: var(--selected); }
  .q b { font-weight: 600; }
  .q .dim { font-size: 12.5px; }
  .q .sz { color: var(--text-2); font-size: 12.5px; }
  .radio { width: 14px; height: 14px; border-radius: 50%; border: 1px solid var(--border-strong); background: var(--surface); transition: box-shadow 120ms var(--ease-out), border-color 120ms; }
  .q.on .radio { border-color: var(--accent); box-shadow: inset 0 0 0 3.5px var(--accent); }
  .hint { display: flex; align-items: flex-start; gap: 6px; color: var(--text-3); font-size: 12px; }
  .hint :global(.ic) { margin-top: 2px; }

  details { border-top: 1px solid var(--border-subtle); padding-top: 12px; }
  summary { cursor: pointer; color: var(--text-2); font-weight: 500; list-style: none; display: flex; align-items: center; gap: 6px; }
  summary::before { content: ""; width: 6px; height: 6px; border-right: 1.5px solid currentColor; border-bottom: 1.5px solid currentColor; transform: rotate(-45deg); transition: transform 150ms var(--ease-out); margin: 0 4px 0 2px; }
  details[open] summary::before { transform: rotate(45deg); }
  .adv { display: flex; flex-direction: column; gap: 10px; padding-top: 12px; }
  .arow { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .arow.col { flex-direction: column; align-items: stretch; }
  .toggle { display: flex; align-items: center; gap: 10px; color: var(--text); height: 28px; text-align: left; }
  .langs { display: flex; flex-wrap: wrap; gap: 4px; max-height: 80px; overflow-y: auto; }
  .langs .badge { cursor: pointer; }

  .lrow { display: flex; align-items: center; justify-content: space-between; }
  .plist { max-height: 240px; overflow-y: auto; border: 1px solid var(--border); border-radius: var(--r-2); }
  .pe { width: 100%; display: flex; align-items: center; gap: 10px; height: 32px; padding: 0 10px; border-bottom: 1px solid var(--border-subtle); text-align: left; color: var(--text-3); }
  .pe:last-child { border-bottom: 0; }
  .pe:hover { background: var(--hover); }
  .pe.on { color: var(--text); }
  .pi { width: 24px; text-align: right; font-size: 12px; }
  .pt { flex: 1; }

  .file { display: flex; gap: 12px; align-items: flex-end; }
  .fic { width: 40px; height: 40px; border-radius: var(--r-2); background: var(--surface-2); border: 1px solid var(--border-subtle); display: grid; place-items: center; color: var(--text-2); flex: none; }
  .fi { flex: 1; display: flex; flex-direction: column; gap: 4px; }
  dl { display: grid; grid-template-columns: 72px 1fr; gap: 6px 12px; font-size: 12.5px; }
  dt { color: var(--text-3); }
  dd { min-width: 0; }
  dd.ok { color: var(--ok); }
</style>
