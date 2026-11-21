<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import { call } from "../lib/api";
  import { store, ACTIVE } from "../lib/store.svelte";
  import { bytes, speed, eta } from "../lib/fmt";

  let cv: HTMLCanvasElement;
  let free = $state(0);
  const active = $derived(store.tasks.filter((t) => ACTIVE.has(t.status)).length);
  const queued = $derived(store.tasks.filter((t) => t.status === "queued").length);

  onMount(() => {
    const getFree = async () => (free = await call("free_space").catch(() => 0));
    getFree();
    const iv = setInterval(getFree, 30_000);
    // son 60 örnekten çubuk grafik, son çubuk vurgu renginde. sadece veri değişince çiz
    const ctx = cv.getContext("2d")!;
    let last = "";
    let raf = 0;
    const draw = () => {
      raf = requestAnimationFrame(draw);
      if (document.hidden) return;
      const data = store.snap.history.slice(-60);
      const key = data.length + ":" + (data[data.length - 1] ?? 0) + ":" + document.documentElement.dataset.theme;
      if (key === last) return;
      last = key;
      const w = cv.clientWidth, h = cv.clientHeight, dpr = devicePixelRatio || 1;
      if (cv.width !== Math.round(w * dpr)) { cv.width = Math.round(w * dpr); cv.height = Math.round(h * dpr); }
      const css = getComputedStyle(document.documentElement);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, w, h);
      const n = 40, bar = 2, gap = (w - n * bar) / (n - 1);
      const src = data.slice(-n);
      while (src.length < n) src.unshift(0);
      const max = Math.max(256 * 1024, ...src);
      for (let i = 0; i < n; i++) {
        const bh = src[i] > 0 ? Math.max(1.5, (src[i] / max) * h) : 1;
        ctx.fillStyle = i === n - 1 && src[i] > 0 ? css.getPropertyValue("--accent") : css.getPropertyValue(src[i] > 0 ? "--text-3" : "--border");
        ctx.fillRect(Math.round(i * (bar + gap)), h - bh, bar, bh);
      }
    };
    draw();
    return () => { clearInterval(iv); cancelAnimationFrame(raf); };
  });
</script>

<footer>
  <div class="spd" class:live={store.snap.speed > 0} aria-label="Toplam hız">
    <Icon n="down" size={13} />
    <b class="num">{store.snap.speed > 0 ? speed(store.snap.speed) : "0 B/sn"}</b>
    <canvas bind:this={cv} aria-hidden="true"></canvas>
  </div>
  <span class="sep"></span>
  <span class="num">{active} indiriliyor</span>
  {#if queued}<span class="num">{queued} sırada</span>{/if}
  {#if store.snap.eta_total > 0}<span class="num">tümü için {eta(store.snap.eta_total)}</span>{/if}
  <span class="fill"></span>
  {#if store.settings?.speed_limit}<span class="num">Hız sınırı {bytes(store.settings.speed_limit * 1024)}/sn</span>{/if}
  {#if store.snap.session}<span class="num">Bu oturumda {bytes(store.snap.session)}</span>{/if}
  {#if free}<span class="num" class:low={free < 2 * 1024 ** 3}><Icon n="disk" size={13} />{bytes(free)} boş</span>{/if}
</footer>

<style>
  footer { height: 28px; flex: none; display: flex; align-items: center; gap: 16px; padding: 0 12px; font-size: 12px; color: var(--text-2); background: var(--bg-sunken); border-top: 1px solid var(--border-subtle); white-space: nowrap; overflow: hidden; }
  footer span { display: flex; align-items: center; gap: 5px; }
  .spd { display: flex; align-items: center; gap: 6px; color: var(--text-3); }
  .spd b { font-weight: 600; color: var(--text-2); min-width: 72px; }
  .spd.live { color: var(--accent); }
  .spd.live b { color: var(--text); }
  canvas { width: 96px; height: 14px; display: block; }
  .sep { width: 1px; height: 14px; background: var(--border); }
  .fill { flex: 1; }
  .low { color: var(--error); }
</style>
