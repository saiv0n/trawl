<script lang="ts">
  // dolgu scaleX ile, güncelleme aralığı boyunca doğrusal. ease-out her tıkta fırlayıp duruyor
  // çok bağlantılıda her parça kendi yerinde dolar, aralarında 1 px boşluk
  // etkinken dolgunun tersine akan çizgiler, bekleme daha kısa hissettiriyor
  let {
    pct = 0,
    segs = [],
    state = "idle",
    height = 4,
    notches = 0,
  }: { pct?: number; segs?: [number, number, number, number][]; state?: "active" | "paused" | "error" | "indet" | "phase" | "done" | "idle"; height?: number; notches?: number } = $props();
  const multi = $derived(segs.length > 1 && state !== "indet" && state !== "phase");
</script>

<div class="track s-{state}" style="height:{height}px" role="presentation">
  {#if state === "indet" || state === "phase"}
    <i class="indet"></i>
  {:else if multi}
    {#each segs as s}
      <span class="seg" style="left:{s[0] * 100}%;width:{(s[1] - s[0]) * 100}%"><i class="fill" style="transform:scaleX({s[2]})"></i></span>
    {/each}
  {:else}
    <i class="fill" style="transform:scaleX({Math.max(0, Math.min(1, pct))})"></i>
  {/if}
  {#if state === "active"}<i class="stripes"></i>{/if}
  {#if notches > 1 && !multi && state !== "indet" && state !== "phase"}<i class="notch" style="--n:{Math.min(notches, 32)}"></i>{/if}
</div>

<style>
  .track { position: relative; width: 100%; border-radius: 2px; background: var(--track); overflow: hidden; contain: strict; }
  .fill { position: absolute; inset: 0; transform-origin: left; background: var(--text-3); transition: transform var(--tick) linear; }
  .s-active .fill { background: var(--accent); }
  .s-error .fill { background: var(--error); }
  .s-done .fill { background: var(--ok); }
  .seg { position: absolute; top: 0; bottom: 0; overflow: hidden; box-shadow: inset -1px 0 0 var(--bg); }
  .seg:last-child { box-shadow: none; }
  .stripes {
    position: absolute; top: 0; bottom: 0; left: -24px; right: 0; pointer-events: none; opacity: 0.16;
    background: repeating-linear-gradient(115deg, transparent 0 5px, var(--surface) 5px 8px);
    animation: back 1.2s linear infinite;
  }
  @keyframes back { from { transform: translateX(0); } to { transform: translateX(-11.6px); } }
  /* listede bağlantı sayısı kadar eşit çentik yeter, parça haritası ayrıntı panelinde */
  .notch { position: absolute; inset: 0; pointer-events: none; background: repeating-linear-gradient(90deg, transparent 0 calc(100% / var(--n) - 1px), var(--bg) calc(100% / var(--n) - 1px) calc(100% / var(--n))); }
  :global(.row.sel) .notch { background: repeating-linear-gradient(90deg, transparent 0 calc(100% / var(--n) - 1px), var(--selected) calc(100% / var(--n) - 1px) calc(100% / var(--n))); }
  :global(.scrolling) .fill { transition: none; }
  .indet { position: absolute; top: 0; bottom: 0; width: 32%; left: 0; background: var(--text-3); border-radius: 2px; animation: indet 1.2s var(--ease-in-out) infinite; }
  .s-indet .indet { background: var(--accent); }
  @keyframes indet { from { transform: translateX(-100%); } to { transform: translateX(320%); } }
</style>
