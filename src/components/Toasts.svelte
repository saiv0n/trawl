<script lang="ts">
  import { fly, fade } from "svelte/transition";
  import { flip } from "svelte/animate";
  import { cubicOut } from "svelte/easing";
  import Icon from "./Icon.svelte";
  import { toasts } from "../lib/store.svelte";
  const IC: Record<string, string> = { ok: "ok", error: "warn", info: "info", clip: "paste", warn: "power" };
</script>

<div class="stack" role="region" aria-label="Bildirimler" aria-live="polite">
  {#each toasts.list as t (t.id)}
    <div
      class="toast k-{t.kind}"
      role="status"
      animate:flip={{ duration: 180, easing: cubicOut }}
      in:fly={{ y: 12, duration: 240, easing: cubicOut }}
      out:fade={{ duration: 150 }}
      onmouseenter={() => toasts.hold(t.id)}
      onmouseleave={() => toasts.release(t.id)}
    >
      <Icon n={IC[t.kind]} size={16} filled={t.kind === "ok" || t.kind === "error"} />
      <div class="tx">
        <b>{t.title}</b>
        {#if t.body}<p class="ellip">{t.body}</p>{/if}
      </div>
      {#each t.actions || [] as a}
        <button class="btn btn-sm" class:btn-primary={a.primary} onclick={() => { a.run(); toasts.close(t.id); }}>{a.label}</button>
      {/each}
      <button class="icon-btn" aria-label="Kapat" onclick={() => toasts.close(t.id)}><Icon n="x" size={13} /></button>
    </div>
  {/each}
</div>

<style>
  .stack { position: fixed; right: 16px; bottom: 40px; z-index: 80; display: flex; flex-direction: column; gap: 8px; width: 400px; max-width: calc(100vw - 32px); pointer-events: none; }
  .toast { pointer-events: auto; display: flex; align-items: center; gap: 10px; padding: 10px 6px 10px 12px; border-radius: var(--r-3); background: var(--surface); box-shadow: var(--shadow-menu), var(--raise); }
  .toast > :global(.ic) { color: var(--text-2); align-self: flex-start; margin-top: 1px; }
  .k-ok > :global(.ic) { color: var(--ok); }
  .k-error > :global(.ic) { color: var(--error); }
  .k-clip > :global(.ic), .k-warn > :global(.ic) { color: var(--accent); }
  .tx { flex: 1; min-width: 0; }
  .tx b { display: block; font-weight: 500; }
  .tx p { font-size: 12px; color: var(--text-3); }
  .icon-btn { width: 24px; height: 24px; flex: none; }
</style>
