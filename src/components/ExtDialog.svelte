<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import { call, inTauri } from "../lib/api";
  import { toasts } from "../lib/store.svelte";
  let { onclose }: { onclose: () => void } = $props();
  let path = $state("");
  // eklenti her ping'de sürüm ve tarayıcı bilgisini yolluyor, 2 sn'de bir bak
  let st = $state<{ ago: number; version: string; browser: string } | null>(null);
  const NAMES: Record<string, string> = { chrome: "Chrome", edge: "Edge", brave: "Brave", opera: "Opera", vivaldi: "Vivaldi", firefox: "Firefox" };
  const live = $derived(!!st && st.ago >= 0 && st.ago < 120);
  const ago = (s: number) => (s < 10 ? "az önce" : s < 60 ? `${s} sn önce` : `${Math.round(s / 60)} dk önce`);
  onMount(() => {
    const tick = async () => (st = await call("ext_status").catch(() => null));
    tick();
    const iv = setInterval(tick, 2000);
    return () => clearInterval(iv);
  });
  const steps = [
    ["Tarayıcının uzantılar sayfasını açın", "chrome://extensions, edge://extensions ya da brave://extensions. Chrome adresini aşağıdan kopyalayabilirsiniz."],
    ["Geliştirici modunu açın", "Sağ üstteki anahtar."],
    ["Paketlenmemiş öğe yükle'ye tıklayın", "Açılan Trawl eklenti klasörünü seçin."],
    ["Hazır", "Videoların üstünde İndir düğmesi çıkar, indirmeler Trawl'a gelir. Trawl kapalıyken bile: eklenti onu kendisi açar."],
  ];
</script>

<div class="ex">
  <header>
    <div><h2>Tarayıcı eklentisi</h2><p>Sayfada çalan videoyu ve sesi yakalar, tarayıcıdaki indirmeleri Trawl'a devreder.</p></div>
    <button class="icon-btn" aria-label="Kapat" onclick={onclose}><Icon n="x" /></button>
  </header>
  <div class="st" class:on={live} role="status">
    <i></i>
    {#if live && st}
      <span><b>Bağlı</b> · {NAMES[st.browser] || st.browser} · eklenti {st.version} · {ago(st.ago)}</span>
    {:else if st && st.ago >= 0}
      <span><b>Bağlantı yok</b> · son görülme {ago(st.ago)}. Tarayıcı açıksa eklentiyi yeniden yükleyin.</span>
    {:else}
      <span><b>Henüz bağlanmadı</b> · aşağıdaki adımlarla kurun.</span>
    {/if}
  </div>
  <ol>
    {#each steps as [t, d], i}
      <li><span class="n num">{i + 1}</span><div><b>{t}</b><small>{d}</small></div></li>
    {/each}
  </ol>
  <footer>
    <span class="mono ellip path">{path || "Bağlantı noktası 127.0.0.1:47321"}</span>
    <button class="btn" onclick={() => { call("write_clipboard", { text: "chrome://extensions" }); toasts.push({ kind: "ok", title: "Adres kopyalandı", ttl: 1500 }); }}><Icon n="copy" size={14} />Adresi kopyala</button>
    <button class="btn btn-primary" disabled={!inTauri} onclick={async () => (path = await call("ext_folder"))}><Icon n="folder" size={14} />Klasörü aç</button>
  </footer>
</div>

<style>
  .ex { width: 520px; max-width: 100%; display: flex; flex-direction: column; }
  header { display: flex; gap: 12px; align-items: flex-start; padding: 16px 16px 12px 20px; border-bottom: 1px solid var(--border-subtle); }
  header div { flex: 1; }
  h2 { font-size: 16px; line-height: 22px; font-weight: 600; }
  header p { color: var(--text-2); font-size: 12.5px; margin-top: 2px; }
  ol { list-style: none; padding: 4px 20px; }
  li { display: flex; gap: 14px; padding: 12px 0; border-bottom: 1px solid var(--border-subtle); }
  li:last-child { border-bottom: 0; }
  li div { display: flex; flex-direction: column; gap: 2px; }
  li b { font-weight: 500; }
  li small { color: var(--text-2); font-size: 12.5px; }
  .n { width: 20px; height: 20px; border-radius: 50%; display: grid; place-items: center; font-size: 12px; font-weight: 600; color: var(--accent-text); background: var(--accent-soft); flex: none; }
  footer { display: flex; gap: 8px; align-items: center; padding: 12px 16px 12px 20px; border-top: 1px solid var(--border-subtle); background: var(--bg); }
  .path { flex: 1; color: var(--text-3); }
  .st { display: flex; align-items: center; gap: 10px; margin: 12px 20px 0; padding: 9px 12px; border-radius: var(--r-2); background: var(--surface-2); color: var(--text-2); font-size: 12.5px; }
  .st i { width: 8px; height: 8px; border-radius: 50%; background: var(--text-3); flex: none; }
  .st.on i { background: var(--ok); box-shadow: 0 0 0 3px color-mix(in oklch, var(--ok) 25%, transparent); }
  .st b { color: var(--text); font-weight: 600; }
</style>
