<script lang="ts">
  // açılış 220 ms ease-out, .96'dan 1'e ölçek ve opaklık, kapanış 160 ms. klavyeyle açıldıysa ölçek yok
  import { fade } from "svelte/transition";
  import type { Snippet } from "svelte";
  import { reducedMotion } from "../lib/store.svelte";
  let { onclose, children }: { onclose: () => void; children: Snippet } = $props();
  const kb = typeof window !== "undefined" && (window as any).__lastInputKeyboard === true;

  function enter(_n: Element) {
    const still = kb || reducedMotion();
    return {
      duration: still ? 100 : 220,
      css: (t: number) => {
        const e = 1 - Math.pow(1 - t, 4);
        return still ? `opacity:${t}` : `opacity:${Math.min(1, t * 1.6)};transform:scale(${0.96 + 0.04 * e})`;
      },
    };
  }
  function leave(_n: Element) {
    return { duration: 160, css: (t: number) => `opacity:${t};transform:scale(${0.98 + 0.02 * t})` };
  }
  let card: HTMLDivElement;
  function trap(e: KeyboardEvent) {
    if (e.key === "Escape") { e.stopPropagation(); onclose(); return; }
    if (e.key !== "Tab" || !card) return;
    const f = [...card.querySelectorAll<HTMLElement>("button, [href], input, select, textarea, [tabindex]:not([tabindex='-1'])")].filter((x) => !x.hasAttribute("disabled"));
    if (!f.length) return;
    const first = f[0], last = f[f.length - 1];
    if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
    else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
  }
</script>

<svelte:window onkeydown={trap} />

<div class="wrap">
  <div class="scrim" transition:fade={{ duration: 160 }} onclick={onclose} role="presentation"></div>
  <div class="card" role="dialog" aria-modal="true" bind:this={card} in:enter out:leave>
    {@render children()}
  </div>
</div>

<style>
  .wrap { position: fixed; inset: 0; z-index: 50; display: grid; place-items: center; padding: 32px; }
  .scrim { position: absolute; inset: 0; background: var(--scrim); }
  .card { position: relative; max-width: 100%; max-height: calc(100vh - 64px); display: flex; flex-direction: column; border-radius: var(--r-3); overflow: hidden; background: var(--surface); box-shadow: var(--shadow-dialog), var(--raise); }
</style>
