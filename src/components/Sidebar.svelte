<script lang="ts">
  import { tick } from "svelte";
  import Icon from "./Icon.svelte";
  import { store, CATS } from "../lib/store.svelte";
  import { t } from "../lib/i18n.svelte";

  // seçim göstergesi görünüm değişince kayar, ilk ölçümde kaymasın
  let nav: HTMLElement;
  let ind = $state({ y: 0, h: 0, show: false, anim: false });
  function measure() {
    const b = nav?.querySelector<HTMLElement>("button.on");
    ind = b ? { y: b.offsetTop, h: b.offsetHeight, show: true, anim: ind.show } : { ...ind, show: false };
  }
  $effect(() => {
    void store.filter;
    void store.count("error"); // "İndirilemeyen" çıkıp kaybolunca öğeler kayıyor, onu da izle
    tick().then(measure);
  });

  const views = [
    { f: "all", label: "Tüm indirmeler", icon: "list" },
    { f: "active", label: "İndiriliyor", icon: "down" },
    { f: "queued", label: "Sırada", icon: "clock" },
    { f: "done", label: "Tamamlanan", icon: "ok" },
    { f: "error", label: "İndirilemeyen", icon: "warn" },
  ];
  const cats = Object.entries(CATS).map(([f, c]) => ({ f, label: c.label, icon: c.icon }));
</script>

<aside aria-label={t("Görünümler")}>
  <button class="btn btn-primary add" onclick={() => (store.modal = { type: "add" })}>
    <Icon n="plus" size={14} />{t("Bağlantı ekle")}<kbd>Ctrl N</kbd>
  </button>

  <nav bind:this={nav}>
    <i class="ind" class:anim={ind.anim} class:show={ind.show} style="transform:translateY({ind.y}px);height:{ind.h}px" aria-hidden="true"></i>
    {#each views as it, i}
      {@const n = store.count(it.f)}
      {#if it.f !== "error" || n}
        <button class:on={store.filter === it.f} aria-current={store.filter === it.f} onclick={() => (store.filter = it.f)} style="--i:{i}">
          <Icon n={it.icon} size={16} filled={store.filter === it.f} />
          <span class="ellip">{t(it.label)}</span>
          {#if n}<em class="num" class:err={it.f === "error"}>{n}</em>{/if}
        </button>
      {/if}
    {/each}
    <div class="group">{t("Tür")}</div>
    {#each cats as it, i}
      {@const n = store.count(it.f)}
      <button class:on={store.filter === it.f} aria-current={store.filter === it.f} onclick={() => (store.filter = it.f)} style="--i:{i + 6}">
        <Icon n={it.icon} size={16} filled={store.filter === it.f} />
        <span class="ellip">{t(it.label)}</span>
        {#if n}<em class="num">{n}</em>{/if}
      </button>
    {/each}
  </nav>

  <div class="foot">
    <button onclick={() => (store.modal = { type: "ext" })}><Icon n="puzzle" size={16} /><span>{t("Tarayıcı eklentisi")}</span></button>
    <button onclick={() => (store.modal = { type: "settings" })}><Icon n="gear" size={16} /><span>{t("Ayarlar")}</span><kbd>Ctrl ,</kbd></button>
  </div>
</aside>

<style>
  aside { width: 220px; flex: none; display: flex; flex-direction: column; gap: 10px; padding: 12px 8px 8px; background: var(--bg-sunken); border-right: 1px solid var(--border-subtle); min-height: 0; }
  .add { justify-content: flex-start; margin: 0 2px 4px; }
  .add kbd { margin-left: auto; color: color-mix(in oklch, var(--on-accent) 75%, transparent); border-color: color-mix(in oklch, var(--on-accent) 30%, transparent); background: transparent; }
  nav { flex: 1; min-height: 0; overflow-y: auto; display: flex; flex-direction: column; gap: 1px; position: relative; }
  nav > button { position: relative; z-index: 1; }
  .ind { position: absolute; top: 0; left: 0; right: 0; z-index: 0; border-radius: var(--r-1); background: var(--surface); box-shadow: 0 0 0 1px var(--border-subtle); pointer-events: none; opacity: 0; }
  .ind.show { opacity: 1; }
  .ind.anim { transition: transform 200ms var(--ease-in-out); }
  :global(.motion-reduced) .ind.anim { transition: none; }
  nav button, .foot button { height: 30px; flex: none; display: flex; align-items: center; gap: 10px; padding: 0 8px 0 10px; border-radius: var(--r-1); color: var(--text-2); font-weight: 500; text-align: left; transition: background-color 100ms, color 100ms; }
  nav button:hover, .foot button:hover { background: var(--hover); color: var(--text); }
  nav button.on, nav button.on:hover { background: transparent; color: var(--text); }
  nav button.on :global(.ic) { color: var(--accent); }
  nav em { margin-left: auto; font-style: normal; font-size: 12px; color: var(--text-3); }
  nav em.err { color: var(--error); }
  .group { padding: 14px 10px 6px; font-size: 12px; font-weight: 500; color: var(--text-3); }
  .foot { display: flex; flex-direction: column; gap: 1px; padding-top: 8px; border-top: 1px solid var(--border-subtle); }
  .foot kbd { margin-left: auto; }
</style>
