<script lang="ts">
  import Icon from "./Icon.svelte";
  import { call, inTauri } from "../lib/api";
  import { store } from "../lib/store.svelte";

  let search: HTMLInputElement;
  let maxed = $state(false);
  async function win(a: "min" | "max" | "close") {
    if (!inTauri) return;
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    const w = getCurrentWindow();
    if (a === "min") w.minimize();
    else if (a === "max") { await w.toggleMaximize(); maxed = await w.isMaximized(); }
    else call("close_window");
  }
  export function focusSearch() { search?.focus(); search?.select(); }
</script>

<header data-tauri-drag-region>
  <div class="brand" data-tauri-drag-region>
    <svg class="mark" viewBox="0 0 20 20" aria-hidden="true"><rect width="20" height="20" rx="4.5" fill="var(--text)" /><path d="M3.8 4.4h12.4v2.6h-4.7l-.75 5.6h-1.5l-.75-5.6H3.8z" fill="var(--bg)" /><circle cx="10" cy="14.35" r="2.15" fill="var(--accent)" /></svg>
    <span data-tauri-drag-region>Trawl</span>
  </div>

  <div class="center" data-tauri-drag-region>
    <label class="search">
      <Icon n="search" size={14} />
      <input bind:this={search} bind:value={store.query} placeholder="İndirmelerde ara" spellcheck="false" aria-label="İndirmelerde ara" />
      {#if store.query}
        <button class="clear" aria-label="Aramayı temizle" onclick={() => (store.query = "")}><Icon n="x" size={12} /></button>
      {:else}
        <kbd>Ctrl F</kbd>
      {/if}
    </label>
  </div>

  <div class="caption">
    <button aria-label="Küçült" onclick={() => win("min")}><Icon n="min" size={14} /></button>
    <button aria-label={maxed ? "Geri yükle" : "Büyüt"} onclick={() => win("max")}><Icon n="max" size={12} /></button>
    <button class="close" aria-label="Kapat" onclick={() => win("close")}><Icon n="x" size={15} /></button>
  </div>
</header>

<style>
  header { height: 40px; flex: none; display: grid; grid-template-columns: 220px 1fr auto; align-items: center; background: var(--bg-sunken); border-bottom: 1px solid var(--border-subtle); }
  .brand { display: flex; align-items: center; gap: 9px; padding-left: 14px; font-weight: 600; font-size: 13px; }
  .mark { width: 18px; height: 18px; }
  .center { display: flex; justify-content: center; height: 100%; align-items: center; padding: 0 16px; }
  .search { width: min(440px, 100%); height: 28px; display: flex; align-items: center; gap: 8px; padding: 0 4px 0 10px; border-radius: var(--r-2); background: var(--bg); border: 1px solid var(--border-subtle); color: var(--text-3); transition: border-color 120ms; }
  .search:hover { border-color: var(--border); }
  .search:focus-within { border-color: var(--accent); color: var(--text-2); }
  .search input { flex: 1; min-width: 0; background: none; border: 0; outline: none; font-size: 13px; color: var(--text); }
  .search input::placeholder { color: var(--text-3); }
  .search kbd { margin-right: 2px; }
  .clear { width: 22px; height: 22px; border-radius: var(--r-1); display: grid; place-items: center; color: var(--text-3); }
  .clear:hover { background: var(--hover); color: var(--text); }
  .caption { display: flex; height: 100%; }
  .caption button { width: 46px; height: 100%; display: grid; place-items: center; color: var(--text-2); transition: background-color 100ms; }
  .caption button:hover { background: var(--hover); color: var(--text); }
  .caption .close:hover { background: #c42b1c; color: #fff; }
</style>
