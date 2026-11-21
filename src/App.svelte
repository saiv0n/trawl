<script lang="ts">
  import { onMount, tick } from "svelte";
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import TitleBar from "./components/TitleBar.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import StatusBar from "./components/StatusBar.svelte";
  import TaskRow from "./components/TaskRow.svelte";
  import DetailPanel from "./components/DetailPanel.svelte";
  import Modal from "./components/Modal.svelte";
  import AddDialog from "./components/AddDialog.svelte";
  import Grabber from "./components/Grabber.svelte";
  import Settings from "./components/Settings.svelte";
  import ExtDialog from "./components/ExtDialog.svelte";
  import Toasts from "./components/Toasts.svelte";
  import Icon from "./components/Icon.svelte";
  import { call, on, inTauri } from "./lib/api";
  import { store, toasts, chime, applyLook, watchSystem, reducedMotion, CATS, ACTIVE } from "./lib/store.svelte";
  import { host, bytes, kindOfExt } from "./lib/fmt";

  let tb: TitleBar;
  let omni = $state("");
  let dragging = $state(false);

  // sanal liste
  let scroller = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let scrolling = $state(false);
  let scrollT: any;
  let viewH = $state(600);
  let nameW = $state(260);
  let headName = $state<HTMLElement>();
  const ROW = { compact: 36, default: 44, comfortable: 52 } as Record<string, number>;
  const rowH = $derived(ROW[store.settings?.density || "default"] || 44);
  const list = $derived(store.visible);
  const first = $derived(Math.max(0, Math.floor(scrollTop / rowH) - 6));
  const last = $derived(Math.min(list.length, Math.ceil((scrollTop + viewH) / rowH) + 6));
  const slice = $derived(list.slice(first, last));

  function scrollToId(id: string) {
    const i = list.findIndex((t) => t.id === id);
    if (i < 0 || !scroller) return;
    const top = i * rowH, bottom = top + rowH;
    if (top < scroller.scrollTop) scroller.scrollTop = top;
    else if (bottom > scroller.scrollTop + viewH) scroller.scrollTop = bottom - viewH;
  }

  const TITLES: Record<string, string> = { all: "Tüm indirmeler", active: "İndiriliyor", queued: "Sırada", done: "Tamamlanan", error: "İndirilemeyen" };
  const title = $derived(TITLES[store.filter] || CATS[store.filter]?.label || "");
  const selIds = $derived([...store.selected].filter((id) => store.tasks.some((t) => t.id === id)));
  const missingTools = $derived(store.tools && (!store.tools.ytdlp || !store.tools.ffmpeg));
  const anyActive = $derived(store.tasks.some((t) => ACTIVE.has(t.status)));
  const anyPaused = $derived(store.tasks.some((t) => t.status === "paused"));
  const anyDone = $derived(store.tasks.some((t) => t.status === "done" && !store.hidden.has(t.id)));

  // eylemler
  function openAdd(url = "", headers?: Record<string, string>) {
    store.modal = { type: "add", url, headers };
  }
  function submitOmni(e: Event) {
    e.preventDefault();
    if (omni.trim()) { openAdd(omni.trim()); omni = ""; }
  }
  function bulk(cmd: "pause" | "resume") {
    for (const id of selIds) call(cmd, { id });
  }
  function clearDone() {
    store.remove(store.tasks.filter((t) => t.status === "done" && !store.hidden.has(t.id)).map((t) => t.id));
  }
  async function confirmDelete(ids: string[]) {
    store.modal = null;
    for (const id of ids) await call("remove", { id, deleteFile: true });
    const sel = new Set(store.selected);
    ids.forEach((i) => sel.delete(i));
    store.selected = sel;
    toasts.push({ kind: "ok", title: ids.length === 1 ? "Dosya Geri Dönüşüm Kutusu'na taşındı" : `${ids.length} dosya Geri Dönüşüm Kutusu'na taşındı` });
  }
  let newUrl = $state("");
  let urlErr = $state("");
  let urlBusy = $state(false);
  async function updateUrl(id: string) {
    urlBusy = true; urlErr = "";
    try {
      await call("update_url", { id, url: newUrl.trim() });
      store.modal = null;
      newUrl = "";
      toasts.push({ kind: "ok", title: "Bağlantı güncellendi, indirme kaldığı yerden sürüyor" });
    } catch (e) { urlErr = String(e); }
    urlBusy = false;
  }
  const delInfo = $derived.by(() => {
    if (store.modal?.type !== "confirm-delete") return { n: 0, size: 0 };
    const ids = store.modal.ids;
    const ts = store.tasks.filter((t) => ids.includes(t.id));
    return { n: ts.length, size: ts.reduce((a, t) => a + (t.status === "done" ? t.size : t.done), 0) };
  });

  // olaylar
  let lastDoneAt = 0;
  function notice(n: any) {
    switch (n.type) {
      case "done": {
        // aynı saniyede birkaç tane biterse animasyonu sadece ilki oynatsın
        const now = Date.now();
        if (now - lastDoneAt > 1000) {
          chime();
          store.justDone = new Set([...store.justDone, n.id]);
          setTimeout(() => { const s = new Set(store.justDone); s.delete(n.id); store.justDone = s; }, 1100);
        }
        lastDoneAt = now;
        if (!document.hasFocus())
          toasts.push({ kind: "ok", title: "İndirme tamamlandı", body: n.name, actions: [{ label: "Aç", primary: true, run: () => call("open_task", { id: n.id }) }] });
        break;
      }
      case "error":
        toasts.push({
          kind: "error", title: "İndirilemedi", body: `${n.name ? n.name + ": " : ""}${n.error}`, ttl: 8000,
          actions: n.tools ? [{ label: "Bileşenleri kur", primary: true, run: () => (store.modal = { type: "settings", tab: "bilesen" }) }] : [{ label: "Göster", run: () => { store.filter = "all"; store.select(n.id); store.detail = true; tick().then(() => scrollToId(n.id)); } }],
        });
        break;
      case "refreshed":
        toasts.push({ kind: "ok", title: "Bağlantı yenilendi", body: `${n.name}: kaldığı yerden sürüyor`, ttl: 5000 });
        break;
      case "clips": {
        if (store.modal) return;
        const urls: string[] = n.urls;
        const items = urls.map((u) => {
          const name = decodeURIComponent(u.split(/[?#]/)[0].split("/").pop() || "") || host(u);
          const ext = (name.match(/\.(\w{1,5})$/) || [])[1]?.toLowerCase() || "";
          return { url: u, kind: kindOfExt(ext), name, ext, size: 0, mime: "", source: "pano", thumb: "", title: "", page: "" };
        });
        toasts.push({
          kind: "clip", title: `Panoda ${urls.length} bağlantı var`, body: [...new Set(urls.map(host))].slice(0, 3).join(", "), ttl: 10000,
          actions: [{ label: "Seç ve indir", primary: true, run: () => (store.modal = { type: "grab", data: { url: "", title: "Panodaki bağlantılar", icon: "", pages: 1, items } }) }],
        });
        break;
      }
      case "dup": {
        const show = () => { store.filter = "all"; store.select(n.id); store.detail = true; tick().then(() => scrollToId(n.id)); };
        const again = () => call("add", { p: { kind: "http", url: n.url, name: n.filename, headers: n.headers, page: n.page } });
        if (n.done)
          toasts.push({ kind: "warn", title: "Bu dosya zaten indirildi", body: n.filename || host(n.url), ttl: 10000, actions: [{ label: "Aç", primary: true, run: () => call("open_task", { id: n.id }) }, { label: "Yeniden indir", run: again }] });
        else if (n.status === "paused" || n.status === "error")
          toasts.push({ kind: "warn", title: "Bu indirme zaten listede", body: n.filename || host(n.url), ttl: 10000, actions: [{ label: "Sürdür", primary: true, run: () => call("resume", { id: n.id }) }, { label: "Göster", run: show }] });
        else toasts.push({ kind: "ok", title: "Bu indirme zaten sürüyor", body: n.filename || host(n.url), actions: [{ label: "Göster", run: show }] });
        break;
      }
      case "clip":
        if (store.modal) return;
        toasts.push({ kind: "clip", title: "Panoda bağlantı var", body: n.url, ttl: 8000, actions: [{ label: "İndir", primary: true, run: () => openAdd(n.url) }] });
        break;
      case "external":
        openAdd(n.url, n.headers);
        break;
      case "grab":
        store.modal = { type: "grab", data: n.data };
        break;
      case "after_all":
        toasts.push({ kind: "warn", title: n.action === "shutdown" ? "Bilgisayar 60 sn içinde kapanacak" : "Tüm indirmeler bitti", ttl: 60000, actions: n.action === "shutdown" ? [{ label: "İptal et", primary: true, run: () => call("cancel_shutdown") }] : [] });
        break;
    }
  }

  function typing(e: KeyboardEvent) {
    const el = e.target as HTMLElement;
    return el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable;
  }
  async function pasteAndAdd() {
    const c: string = await call("read_clipboard").catch(() => "");
    if (!/^https?:\/\//.test(c.trim())) return;
    openAdd(c.trim());
    const s = store.settings;
    if (s && !s.clipboard && !s.clipboard_asked) {
      store.saveSettings({ clipboard_asked: true });
      toasts.push({
        kind: "info", title: "Kopyaladığınız bağlantıları otomatik algılayalım mı?", body: "Panoyu yalnızca bağlantı için izler, istediğiniz an kapatırsınız.", ttl: 12000,
        actions: [{ label: "Aç", primary: true, run: () => store.saveSettings({ clipboard: true }) }, { label: "Hayır", run: () => {} }],
      });
    }
  }
  function moveSel(d: number, extend: boolean) {
    if (!list.length) return;
    const cur = store.anchor ? list.findIndex((t) => t.id === store.anchor) : -1;
    const i = Math.max(0, Math.min(list.length - 1, cur < 0 ? 0 : cur + d));
    const id = list[i].id;
    if (extend) store.select(id, { shiftKey: true });
    else store.select(id);
    if (extend) store.anchor = store.anchor; // çapa yerinde kalsın
    scrollToId(id);
  }
  async function keys(e: KeyboardEvent) {
    const k = e.key.toLowerCase();
    // release'te tarayıcı kısayolları kapalı
    if (inTauri && !import.meta.env.DEV && (e.key === "F5" || (e.ctrlKey && (k === "r" || k === "p" || k === "+" || k === "-" || k === "=" || k === "0")))) {
      e.preventDefault();
      if (!(e.ctrlKey && k === "p")) return;
    }
    if (e.ctrlKey && k === "n") { e.preventDefault(); openAdd(); return; }
    if (e.ctrlKey && k === "f") { e.preventDefault(); tb.focusSearch(); return; }
    if (e.ctrlKey && e.key === ",") { e.preventDefault(); store.modal = { type: "settings" }; return; }
    if (store.modal || typing(e)) return;
    if (e.ctrlKey && k === "v") { e.preventDefault(); pasteAndAdd(); }
    else if (e.ctrlKey && k === "a") { e.preventDefault(); store.selectAll(); }
    else if (e.key === "ArrowDown" || e.key === "ArrowUp") { e.preventDefault(); moveSel(e.key === "ArrowDown" ? 1 : -1, e.shiftKey); }
    else if (e.key === "Home" || e.key === "End") { e.preventDefault(); if (list.length) { const id = list[e.key === "Home" ? 0 : list.length - 1].id; store.select(id); scrollToId(id); } }
    else if (e.key === "Escape") { store.ctx = null; store.clearSelection(); }
    else if (!selIds.length) return;
    else if (e.key === "Delete") { e.preventDefault(); e.shiftKey ? (store.modal = { type: "confirm-delete", ids: selIds }) : store.remove(selIds); }
    else if (e.key === " " || (e.ctrlKey && k === "p")) {
      e.preventDefault();
      const anyRun = selIds.some((id) => ACTIVE.has(store.tasks.find((t) => t.id === id)?.status || ""));
      bulk(anyRun ? "pause" : "resume");
    } else if (e.key === "Enter") {
      const t = store.focused;
      if (t?.status === "done") call("open_task", { id: t.id }).catch((x) => toasts.push({ kind: "error", title: "Açılamadı", body: String(x) }));
      else store.detail = !store.detail;
    }
  }

  function drop(e: DragEvent) {
    e.preventDefault();
    dragging = false;
    const u = e.dataTransfer?.getData("text/uri-list") || e.dataTransfer?.getData("text/plain") || "";
    const firstUrl = u.split(/\r?\n/).find((x) => /^https?:\/\//.test(x.trim()));
    if (firstUrl) openAdd(firstUrl.trim());
  }

  const ctxTask = $derived(store.ctx ? store.tasks.find((t) => t.id === store.ctx!.id) : null);
  function ctxRun(f: () => void) { f(); store.ctx = null; }

  onMount(() => {
    // açılış animasyonu, sadece soğuk açılışta. girdiyi engellemiyor
    document.documentElement.classList.add("launching");
    setTimeout(() => document.documentElement.classList.remove("launching"), 1000);
    let mockIv: any;
    const ro = new ResizeObserver(() => {
      if (scroller) viewH = scroller.clientHeight;
      if (headName) nameW = Math.max(80, headName.clientWidth - 2);
    });
    (async () => {
      store.settings = await call("get_settings");
      applyLook(store.settings!);
      watchSystem(() => store.settings);
      store.ingest(await call("snapshot"));
      store.tools = await call("tools_status").catch(() => null);
      await on("tick", (s: any) => store.ingest(s));
      await on("notice", notice);
      // eklenti site kuralını değiştirdiyse buradaki kopya eski kalmasın, yoksa sonraki kayıt üstüne yazar
      await on("settings-changed", (s: any) => (store.settings = s));
      if (!inTauri) mockIv = setInterval(async () => store.ingest(await call("snapshot")), 250);
      await tick();
      if (scroller) ro.observe(scroller);
      if (headName) ro.observe(headName);
      requestAnimationFrame(() => call("ui_ready"));
    })();
    return () => { clearInterval(mockIv); ro.disconnect(); };
  });
</script>

<svelte:window
  onkeydown={keys}
  onclick={() => (store.ctx = null)}
  onblur={() => (store.ctx = null)}
  ondragenter={(e) => { if (e.dataTransfer?.types.includes("text/uri-list") || e.dataTransfer?.types.includes("text/plain")) dragging = true; }}
  ondragover={(e) => e.preventDefault()}
  ondrop={drop}
/>

<div class="app">
  <TitleBar bind:this={tb} />
  <div class="body">
    <Sidebar />
    <main>
      <form class="toolbar" onsubmit={submitOmni}>
        <label class="url">
          <Icon n="link" size={15} />
          <input bind:value={omni} placeholder="Bağlantı yapıştırın: video, oynatma listesi, dosya ya da herhangi bir sayfa" spellcheck="false" autocomplete="off" aria-label="Bağlantı" />
          <button type="button" class="icon-btn" aria-label="Panodan yapıştır" data-tip="Panodan yapıştır" onclick={async () => (omni = (await call("read_clipboard").catch(() => "")) || omni)}><Icon n="paste" size={15} /></button>
        </label>
        <button type="submit" class="btn btn-primary" disabled={!omni.trim()}><Icon n="down" size={14} />İndir</button>
        <button type="button" class="icon-btn" class:on={store.detail} aria-label="Ayrıntı panelini aç/kapat" aria-pressed={store.detail} data-tip="Ayrıntılar (Enter)" onclick={() => (store.detail = !store.detail)}><Icon n="panel" size={16} /></button>
      </form>

      {#if missingTools}
        <div class="notice">
          <Icon n="box" size={16} />
          <p><b>Video siteleri için bileşenler kurulu değil.</b> yt-dlp, ffmpeg ve deno yaklaşık 150 MB; uygulama klasörüne kurulur.</p>
          <button class="btn btn-sm" onclick={() => (store.modal = { type: "settings", tab: "bilesen" })}>Kur</button>
        </div>
      {/if}

      <div class="listhead">
        {#if selIds.length > 1}
          <div class="bulk">
            <b class="num">{selIds.length} indirme seçildi</b>
            <button class="btn btn-sm" onclick={() => bulk("resume")}><Icon n="play" size={13} />Sürdür</button>
            <button class="btn btn-sm" onclick={() => bulk("pause")}><Icon n="pause" size={13} />Duraklat</button>
            <button class="btn btn-sm" onclick={() => store.remove(selIds)}><Icon n="x" size={13} />Listeden kaldır</button>
            <button class="btn btn-sm btn-danger" onclick={() => (store.modal = { type: "confirm-delete", ids: selIds })}><Icon n="trash" size={13} />Diskten sil</button>
            <button class="btn btn-ghost btn-sm" onclick={() => store.clearSelection()}>Seçimi kaldır</button>
          </div>
        {:else}
          <h1>{title} <span class="num">{list.length}</span></h1>
          <div class="acts">
            {#if anyPaused}<button class="btn btn-ghost btn-sm" onclick={() => call("resume_all")}><Icon n="play" size={13} />Tümünü sürdür</button>{/if}
            {#if anyActive}<button class="btn btn-ghost btn-sm" onclick={() => call("pause_all")}><Icon n="pause" size={13} />Tümünü duraklat</button>{/if}
            {#if anyDone}<button class="btn btn-ghost btn-sm" onclick={clearDone}><Icon n="check" size={13} />Bitenleri kaldır</button>{/if}
          </div>
        {/if}
      </div>

      <div class="workspace">
        <div class="table" role="grid" aria-multiselectable="true" aria-rowcount={list.length}>
          <div class="thead" role="row" class:gone={!store.tasks.length}>
            <span></span><span bind:this={headName}>Ad</span><span>Durum</span><span>İlerleme</span><span class="r">Boyut</span><span class="r">Hız</span><span class="r">Kalan</span><span></span>
          </div>
          <div class="tbody" class:scrolling bind:this={scroller} onscroll={(e) => { scrollTop = e.currentTarget.scrollTop; scrolling = true; clearTimeout(scrollT); scrollT = setTimeout(() => (scrolling = false), 160); }} onclick={(e) => { if (e.target === e.currentTarget) store.clearSelection(); }} role="presentation">
            {#if list.length}
              <div class="spacer" style="height:{list.length * rowH}px">
                <div class="win" style="transform:translateY({first * rowH}px)">
                  <!-- satırlar konuma göre yeniden kullanılıyor, kaydırınca sadece veri değişir -->
                  {#each slice as t}
                    <TaskRow {t} {nameW} />
                  {/each}
                </div>
              </div>
            {:else if store.query}
              <div class="empty small">
                <b>“{store.query}” ile eşleşen indirme yok.</b>
                <button class="btn btn-sm" onclick={() => (store.query = "")}>Aramayı temizle</button>
              </div>
            {:else if store.filter !== "all" && store.tasks.length}
              <div class="empty small">
                <b>Bu görünümde indirme yok.</b>
                <button class="btn btn-sm" onclick={() => (store.filter = "all")}>Tümünü göster</button>
              </div>
            {:else}
              <div class="empty">
                <div class="drop-hint">
                  <h2>Başlamak için bir bağlantı bırakın</h2>
                  <p>Bağlantıyı buraya sürükleyin ya da <kbd>Ctrl</kbd> <kbd>V</kbd> ile yapıştırın. Video, oynatma listesi, doğrudan dosya ya da içinde dosya olan herhangi bir sayfa olabilir.</p>
                  <div class="row-btns">
                    <button class="btn btn-primary btn-lg" onclick={() => openAdd()}><Icon n="plus" size={14} />Bağlantı ekle</button>
                    <button class="btn btn-ghost btn-lg" onclick={() => (store.modal = { type: "ext" })}>Tarayıcı eklentisini kur</button>
                  </div>
                </div>
              </div>
            {/if}
          </div>
        </div>
        <!-- panel sadece seçim varken, yoksa listeyi boşuna daraltıyor -->
        {#if store.detail && store.focused}
          <div class="dp" in:fly={{ x: 16, duration: reducedMotion() ? 0 : 200, easing: cubicOut }} out:fly={{ x: 16, duration: reducedMotion() ? 0 : 140 }}>
            <DetailPanel t={store.focused} />
          </div>
        {/if}
      </div>
    </main>
  </div>
  <StatusBar />
</div>

{#if dragging}
  <div class="drop" transition:fade={{ duration: 120 }} ondragleave={(e) => { if (!e.relatedTarget) dragging = false; }} role="presentation">
    <div class="dbox"><Icon n="down" size={24} /><b>Bırakın, çözümleyelim</b></div>
  </div>
{/if}

{#if store.ctx && ctxTask}
  {@const t = ctxTask}
  {@const many = store.selected.size > 1}
  <div class="ctx" role="menu" style="left:{Math.min(store.ctx.x, innerWidth - 232)}px;top:{Math.min(store.ctx.y, innerHeight - 330)}px" out:fade={{ duration: 100 }}>
    {#if many}
      <div class="ctx-h num">{store.selected.size} indirme</div>
      <button role="menuitem" onclick={() => ctxRun(() => bulk("resume"))}><Icon n="play" size={14} />Sürdür</button>
      <button role="menuitem" onclick={() => ctxRun(() => bulk("pause"))}><Icon n="pause" size={14} />Duraklat</button>
      <hr />
      <button role="menuitem" onclick={() => ctxRun(() => store.remove(selIds))}><Icon n="x" size={14} />Listeden kaldır<kbd>Del</kbd></button>
      <button role="menuitem" class="danger" onclick={() => ctxRun(() => (store.modal = { type: "confirm-delete", ids: selIds }))}><Icon n="trash" size={14} />Diskten sil<kbd>Shift Del</kbd></button>
    {:else}
      {#if t.status === "done"}<button role="menuitem" onclick={() => ctxRun(() => call("open_task", { id: t.id }))}><Icon n="external" size={14} />Aç<kbd>Enter</kbd></button>{/if}
      <button role="menuitem" onclick={() => ctxRun(() => call("reveal_task", { id: t.id }))}><Icon n="folder" size={14} />Klasörde göster</button>
      {#if ACTIVE.has(t.status) && t.status !== "processing"}<button role="menuitem" onclick={() => ctxRun(() => call("pause", { id: t.id }))}><Icon n="pause" size={14} />Duraklat<kbd>Boşluk</kbd></button>{/if}
      {#if ["paused", "error"].includes(t.status)}<button role="menuitem" onclick={() => ctxRun(() => call("resume", { id: t.id }))}><Icon n="play" size={14} />Sürdür<kbd>Boşluk</kbd></button>{/if}
      {#if t.status === "queued" || t.status === "paused"}<button role="menuitem" onclick={() => ctxRun(() => call("start_now", { id: t.id }))}><Icon n="bolt" size={14} />Hemen başlat</button>{/if}
      {#if !ACTIVE.has(t.status) && t.status !== "done"}<button role="menuitem" onclick={() => ctxRun(() => (store.modal = { type: "update-url", id: t.id }))}><Icon n="link" size={14} />Bağlantıyı güncelle</button>{/if}
      <button role="menuitem" onclick={() => ctxRun(() => call("write_clipboard", { text: t.url }))}><Icon n="copy" size={14} />Bağlantıyı kopyala</button>
      <button role="menuitem" onclick={() => ctxRun(() => call("move_task", { id: t.id, to: 0 }))}><Icon n="top" size={14} />En üste taşı</button>
      {#if !ACTIVE.has(t.status)}<button role="menuitem" onclick={() => ctxRun(() => call("restart", { id: t.id }))}><Icon n="retry" size={14} />Baştan indir</button>{/if}
      <hr />
      <button role="menuitem" onclick={() => ctxRun(() => store.remove([t.id]))}><Icon n="x" size={14} />Listeden kaldır<kbd>Del</kbd></button>
      {#if t.path}<button role="menuitem" class="danger" onclick={() => ctxRun(() => (store.modal = { type: "confirm-delete", ids: [t.id] }))}><Icon n="trash" size={14} />Diskten sil<kbd>Shift Del</kbd></button>{/if}
    {/if}
  </div>
{/if}

{#if store.modal}
  {@const md = store.modal}
  <Modal onclose={() => (store.modal = null)}>
    {#if md.type === "add"}
      <AddDialog url={md.url} headers={md.headers} onclose={() => (store.modal = null)} />
    {:else if md.type === "grab"}
      <Grabber data={{ url: md.data.url, title: md.data.title, icon: md.data.icon || "", pages: 1, items: md.data.items || [] }} onclose={() => (store.modal = null)} standalone />
    {:else if md.type === "settings"}
      <Settings tab={md.tab} onclose={() => (store.modal = null)} />
    {:else if md.type === "ext"}
      <ExtDialog onclose={() => (store.modal = null)} />
    {:else if md.type === "confirm-delete"}
      <div class="dialog" role="alertdialog" aria-labelledby="del-t">
        <h2 id="del-t">{delInfo.n === 1 ? "Dosya diskten silinsin mi?" : `${delInfo.n} dosya diskten silinsin mi?`}</h2>
        <p>{delInfo.size ? `${bytes(delInfo.size)} yer açılacak. ` : ""}Dosyalar Geri Dönüşüm Kutusu'na taşınır, oradan geri alabilirsiniz.</p>
        <div class="dfoot">
          <button class="btn" onclick={() => (store.modal = null)}>Vazgeç</button>
          <button class="btn btn-primary danger-fill" onclick={() => confirmDelete(md.ids)}>Dosyaları sil</button>
        </div>
      </div>
    {:else if md.type === "update-url"}
      {@const t = store.tasks.find((x) => x.id === md.id)}
      <form class="dialog" onsubmit={(e) => { e.preventDefault(); updateUrl(md.id); }}>
        <h2>Bağlantıyı güncelle</h2>
        <p>Süresi dolan bağlantının yerine yenisini yapıştırın. Aynı dosyaysa inen {t ? bytes(t.done) : ""} korunur ve kaldığı yerden sürer.</p>
        <p class="src mono ellip">{t ? host(t.url) : ""} · {t?.name}</p>
        <!-- svelte-ignore a11y_autofocus -->
        <input class="field mono" bind:value={newUrl} placeholder="https://" autofocus spellcheck="false" aria-label="Yeni bağlantı" />
        {#if urlErr}<p class="ferr" role="alert">{urlErr}</p>{/if}
        <div class="dfoot">
          <button type="button" class="btn" onclick={() => (store.modal = null)}>Vazgeç</button>
          <button type="submit" class="btn btn-primary" disabled={!newUrl.trim() || urlBusy}>{#if urlBusy}<span class="spinner"></span>{/if}Güncelle ve sürdür</button>
        </div>
      </form>
    {/if}
  </Modal>
{/if}

<Toasts />

<style>
  .app { height: 100vh; display: flex; flex-direction: column; --gap: 12px; }
  :global([data-density="compact"]) .app { --row: 36px; }
  :global([data-density="comfortable"]) .app { --row: 52px; }
  .body { flex: 1; display: flex; min-height: 0; }
  main { flex: 1; min-width: 0; display: flex; flex-direction: column; background: var(--bg); }

  .toolbar { display: flex; align-items: center; gap: 8px; padding: 12px 16px; }
  .url { flex: 1; display: flex; align-items: center; gap: 8px; height: 36px; padding: 0 4px 0 12px; border-radius: var(--r-2); background: var(--surface); border: 1px solid var(--border); color: var(--text-3); transition: border-color 120ms, box-shadow 120ms; }
  .url:hover { border-color: var(--border-strong); }
  .url:focus-within { border-color: var(--accent); box-shadow: 0 0 0 3px color-mix(in oklch, var(--focus) 22%, transparent); color: var(--text-2); }
  .url input { flex: 1; min-width: 0; border: 0; background: none; outline: none; font: 12.5px var(--mono); color: var(--text); }
  .url input::placeholder { font-family: var(--sans); font-size: 13px; color: var(--text-3); }
  .toolbar > .btn { height: 36px; }
  .toolbar .icon-btn { width: 36px; height: 36px; }
  .toolbar .icon-btn.on { color: var(--accent); background: var(--accent-soft); }

  .notice { display: flex; align-items: center; gap: 10px; margin: 0 16px 10px; padding: 8px 8px 8px 12px; border-radius: var(--r-2); background: var(--accent-soft); color: var(--accent-text); }
  .notice p { flex: 1; color: var(--text-2); font-size: 12.5px; }
  .notice b { color: var(--text); font-weight: 600; }

  .listhead { height: 40px; display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 0 16px; }
  h1 { font-size: 16px; font-weight: 600; display: flex; align-items: baseline; gap: 8px; }
  h1 span { font-size: 13px; font-weight: 400; color: var(--text-3); }
  .acts, .bulk { display: flex; align-items: center; gap: 4px; }
  .bulk b { font-weight: 600; margin-right: 8px; }

  .workspace { flex: 1; min-height: 0; display: flex; border-top: 1px solid var(--border-subtle); }
  .dp { display: flex; flex: none; min-height: 0; }
  .table { flex: 1; min-width: 0; display: flex; flex-direction: column; container-type: inline-size; }
  .thead { display: grid; grid-template-columns: var(--cols); gap: var(--gap); height: 30px; align-items: center; padding: 0 8px 0 14px; font-size: 12px; font-weight: 500; color: var(--text-3); border-bottom: 1px solid var(--border-subtle); flex: none; }
  .thead .r { text-align: right; }
  .thead.gone { display: none; }
  .tbody { flex: 1; overflow-y: auto; position: relative; }
  .spacer { position: relative; }
  .win { position: absolute; left: 0; right: 0; top: 0; will-change: transform; }

  .thead, .tbody { --cols: 18px minmax(0, 1fr) 132px 168px 132px 92px 76px 92px; }
  /* daralınca önce boyut gizlenir (satırın alt satırına iner), hız ve kalan süre en son */
  @container (width < 940px) { .thead, .tbody { --cols: 18px minmax(0, 1fr) 124px 148px 0px 88px 76px 92px; } }
  @container (width < 820px) { .thead, .tbody { --cols: 18px minmax(0, 1fr) 116px 132px 0px 0px 0px 80px; } }
  .thead > span { overflow: hidden; white-space: nowrap; }

  .empty { height: 100%; display: grid; place-items: center; padding: 32px; }
  .empty.small { display: flex; flex-direction: column; gap: 12px; color: var(--text-2); }
  .empty.small b { font-weight: 500; }
  .drop-hint { width: min(560px, 100%); padding: 40px 32px; border: 1.5px dashed var(--border-strong); border-radius: var(--r-3); text-align: center; display: flex; flex-direction: column; align-items: center; gap: 12px; }
  .drop-hint h2 { font-size: 24px; line-height: 30px; font-weight: 620; letter-spacing: -0.015em; }
  .drop-hint p { color: var(--text-2); max-width: 420px; }
  .row-btns { display: flex; gap: 8px; margin-top: 8px; }

  .drop { position: fixed; inset: 0; z-index: 90; display: grid; place-items: center; background: var(--scrim); }
  .dbox { width: 320px; padding: 32px; border: 2px dashed var(--accent); border-radius: var(--r-3); background: var(--surface); display: flex; flex-direction: column; align-items: center; gap: 8px; color: var(--accent-text); box-shadow: var(--shadow-dialog); }
  .dbox b { color: var(--text); font-size: 15px; font-weight: 600; }

  .ctx { position: fixed; z-index: 70; width: 224px; padding: 4px; border-radius: var(--r-3); background: var(--surface); box-shadow: var(--shadow-menu); }
  .ctx-h { padding: 6px 8px 4px; font-size: 12px; color: var(--text-3); }
  .ctx button { width: 100%; height: 28px; display: flex; align-items: center; gap: 10px; padding: 0 8px; border-radius: var(--r-1); color: var(--text); text-align: left; }
  .ctx button :global(.ic) { color: var(--text-2); }
  .ctx button:hover, .ctx button:focus-visible { background: var(--hover); outline: none; }
  .ctx button.danger, .ctx button.danger :global(.ic) { color: var(--error); }
  .ctx kbd { margin-left: auto; border: 0; background: none; padding: 0; }
  .ctx hr { border: 0; height: 1px; background: var(--border-subtle); margin: 4px 0; }

  .dialog { width: 440px; max-width: 100%; padding: 20px; display: flex; flex-direction: column; gap: 10px; }
  .dialog h2 { font-size: 16px; line-height: 22px; font-weight: 600; }
  .dialog p { color: var(--text-2); }
  .dialog .src { font-size: 12px; color: var(--text-3); }
  .dialog .field { height: 36px; margin-top: 4px; font-size: 12.5px; }
  .ferr { color: var(--error) !important; font-size: 12.5px; }
  .dfoot { display: flex; justify-content: flex-end; gap: 8px; margin-top: 10px; }
  .danger-fill { background: var(--error); border-color: var(--error); color: var(--on-accent); }
  .danger-fill:hover { background: color-mix(in oklch, var(--error) 88%, black); border-color: transparent; }
</style>
