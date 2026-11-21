<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import Icon from "./Icon.svelte";
  import { call, on, inTauri } from "../lib/api";
  import { store, toasts } from "../lib/store.svelte";
  import { bytes, speed } from "../lib/fmt";
  import { t as tr } from "../lib/i18n.svelte";

  let { tab = "genel", onclose }: { tab?: string; onclose: () => void } = $props();
  // svelte-ignore state_referenced_locally
  let cur = $state(tab);
  let q = $state("");
  const s = $derived(store.settings!);
  let saved = $state(0);
  async function set(p: Record<string, any>) {
    await store.saveSettings(p);
    saved = Date.now();
  }

  const TABS = [["genel", "Genel"], ["indirme", "İndirme"], ["medya", "Video ve ses"], ["gorunum", "Görünüm"], ["tarayici", "Tarayıcı"], ["kurallar", "Klasör ve giriş"], ["bilesen", "Bileşenler"], ["hakkinda", "Hakkında"]];
  // ayar araması için her sekmenin anahtar kelimeleri
  const KEYS: Record<string, string> = {
    genel: "klasör konum türe göre pano tepsi windows başlat sürdür bildirim ses bitince uyku kapat",
    indirme: "bağlantı aynı anda hız sınır sunucu saat gündüz proxy zamanlayıcı",
    kurallar: "kural klasör uzantı site giriş kullanıcı şifre parola yetki",
    medya: "video kalite kapsayıcı mp4 mkv ses biçim mp3 bit hızı çerez tarayıcı oturum yaş",
    gorunum: "tema açık koyu sistem vurgu renk hareket animasyon yoğunluk satır dil language",
    tarayici: "eklenti chrome devral alt sınır sor tür uzantı site alt tık",
    bilesen: "yt-dlp ffmpeg deno bileşen kur güncelle otomatik",
    hakkinda: "sürüm lisans",
  };
  const shown = $derived(q.trim() ? TABS.filter(([k, l]) => `${l} ${tr(l)} ${KEYS[k]}`.toLocaleLowerCase("tr").includes(q.trim().toLocaleLowerCase("tr"))) : TABS);
  $effect(() => { if (shown.length && !shown.some(([k]) => k === cur)) cur = shown[0][0]; });

  const LIMITS = [0, 256, 512, 1024, 2048, 5120, 10240, 25600, 51200];
  const ASK = [0, 10, 50, 100, 250, 500, 1024, 2048];
  const DEFAULT_EXTS = "zip rar 7z tar gz tgz bz2 xz zst cab iso img bin exe msi msix appx apk xapk dmg pkg deb rpm appimage jar mp4 mkv avi mov wmv flv webm m4v mpg mpeg 3gp ts mp3 m4a aac flac wav ogg opus wma pdf epub mobi djvu doc docx xls xlsx ppt pptx odt ods odp psd ai torrent";
  const HOSTC = [4, 8, 16, 32, 0];
  const CMD_EX = "Örnek: copy {path} D:\\Yedek";
  const kb = (v: number) => (v ? speed(v * 1024) : tr("Sınırsız"));
  const setRule = (i: number, p: Record<string, string>) => set({ rules: s.rules.map((r, j) => (j === i ? { ...r, ...p } : r)) });
  const setLogin = (i: number, p: Record<string, string>) => set({ logins: s.logins.map((r, j) => (j === i ? { ...r, ...p } : r)) });
  async function ruleFolder(i: number) {
    if (!inTauri) return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const r = await open({ directory: true, defaultPath: s.rules[i].folder || s.folder });
    if (typeof r === "string") setRule(i, { folder: r });
  }
  const limitLabel = $derived(kb(s.speed_limit));

  let prog = $state<Record<string, { done: number; total: number; stage?: string }>>({});
  let busy = $state("");
  let off: (() => void) | undefined;
  onMount(async () => {
    store.tools = await call("tools_status").catch(() => store.tools);
    off = await on("tools-progress", (p: any) => (prog = { ...prog, [p.name]: p }));
  });
  onDestroy(() => off?.());

  async function pick() {
    if (!inTauri) return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const r = await open({ directory: true, defaultPath: s.folder });
    if (typeof r === "string") set({ folder: r });
  }
  async function exportList(all: boolean) {
    if (!inTauri) return;
    const { save } = await import("@tauri-apps/plugin-dialog");
    const path = await save({ defaultPath: "trawl-liste.txt", filters: [{ name: tr("Metin"), extensions: ["txt"] }] });
    if (!path) return;
    try { const n = await call("export_list", { path, all }); toasts.push({ kind: "ok", title: tr("{n} bağlantı kaydedildi", { n }) }); }
    catch (e) { toasts.push({ kind: "error", title: tr("Kaydedilemedi"), body: tr(String(e)) }); }
  }
  async function importList() {
    if (!inTauri) return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const path = await open({ filters: [{ name: tr("Metin"), extensions: ["txt", "lst", "csv"] }] });
    if (typeof path !== "string") return;
    try {
      const text: string = await call("import_list", { path });
      if (!/https?:\/\//i.test(text)) { toasts.push({ kind: "error", title: tr("Dosyada bağlantı yok") }); return; }
      store.modal = { type: "add", url: text };
    } catch (e) { toasts.push({ kind: "error", title: tr("Okunamadı"), body: tr(String(e)) }); }
  }
  async function install(which: string) {
    busy = which; prog = {};
    try { await call("tools_install", { which }); store.tools = await call("tools_status"); toasts.push({ kind: "ok", title: tr("Bileşenler hazır") }); }
    catch (e) { toasts.push({ kind: "error", title: tr("Kurulamadı"), body: tr(String(e)) }); }
    busy = "";
  }
  async function autostart(v: boolean) {
    if (inTauri) {
      const a = await import("@tauri-apps/plugin-autostart");
      try { v ? await a.enable() : await a.disable(); } catch (e) { toasts.push({ kind: "error", title: tr("Ayarlanamadı"), body: tr(String(e)) }); return; }
    }
    set({ autostart: v });
  }
  const TOOLS = $derived([
    { k: "ytdlp", n: "yt-dlp", d: "1800'den fazla siteden video ve ses", path: store.tools?.ytdlp, ver: store.tools?.ytdlp_version },
    { k: "ffmpeg", n: "ffmpeg", d: "Birleştirme, dönüştürme, kapak gömme (LGPL derlemesi)", path: store.tools?.ffmpeg, ver: "" },
    { k: "deno", n: "deno", d: "YouTube'un yüksek kalitelerini açar", path: store.tools?.deno, ver: "" },
  ]);
</script>

{#snippet sw(label: string, desc: string, key: string, fn?: (v: boolean) => void)}
  <button class="line" role="switch" aria-checked={(s as any)[key]} onclick={() => (fn ? fn(!(s as any)[key]) : set({ [key]: !(s as any)[key] }))}>
    <div><b>{tr(label)}</b><small>{tr(desc)}</small></div>
    <span class="switch" class:on={(s as any)[key]}></span>
  </button>
{/snippet}

<div class="st">
  <aside>
    <label class="search"><Icon n="search" size={14} /><input bind:value={q} placeholder={tr("Ayarlarda ara")} aria-label={tr("Ayarlarda ara")} /></label>
    <nav>
      {#each shown as [k, l]}
        <button class:on={cur === k} aria-current={cur === k} onclick={() => (cur = k)}>{tr(l)}</button>
      {/each}
    </nav>
    <p class="saved" aria-live="polite">{#key saved}{#if saved}<span><Icon n="check" size={13} />{tr("Kaydedildi")}</span>{/if}{/key}</p>
  </aside>
  <section>
    <header><h2>{tr(TABS.find(([k]) => k === cur)?.[1] || "")}</h2><button class="icon-btn" aria-label={tr("Kapat")} onclick={onclose}><Icon n="x" /></button></header>
    <div class="pane">
      {#if cur === "genel"}
        <div class="group">
          <div class="line">
            <div><b>{tr("İndirme klasörü")}</b><small class="mono ellip">{s.folder}</small></div>
            <div class="rowb"><button class="btn btn-sm" onclick={() => call("open_folder", {})}>{tr("Aç")}</button><button class="btn btn-sm" onclick={pick}>{tr("Değiştir")}</button></div>
          </div>
          {@render sw("Türe göre ayır", "Video, Müzik, Belgeler gibi alt klasörlere koyar", "categorize")}
        </div>
        <div class="group">
          {@render sw("Panoyu izle", "Bir bağlantı kopyalayınca indirmeyi önerir", "clipboard")}
          {@render sw("Kapatınca tepside kal", "Pencere kapansa da indirmeler sürer", "tray")}
          {@render sw("Windows açılınca başlat", "Tepside sessizce başlar", "autostart", autostart)}
          {@render sw("Yarım kalanları sürdür", "Uygulama açılınca kaldığı yerden devam eder", "auto_resume")}
        </div>
        <div class="group">
          <div class="line">
            <div><b>{tr("Liste")}</b><small>{tr("Bağlantıları metin dosyasına kaydet ya da dosyadan topluca ekle")}</small></div>
            <div class="rowb">
              <button class="btn btn-sm" onclick={() => exportList(false)}>{tr("Bitmeyenleri kaydet")}</button>
              <button class="btn btn-sm" onclick={importList}>{tr("Dosyadan ekle")}</button>
            </div>
          </div>
        </div>
        <div class="group">
          {@render sw("Bildirimler", "Pencere arkadayken biten ve hata veren indirmeleri bildirir", "notify")}
          {@render sw("Bitiş sesi", "Kısa bir tık", "sound")}
          {@render sw("Biteni virüs için tara", "Windows Defender ile; tehdit bulursa indirme hatalı sayılır", "scan")}
          {@render sw("Arşivleri aç", "Biten zip, 7z, rar ve tar dosyasını yanındaki klasöre çıkarır", "extract")}
          {#if s.extract}{@render sw("Açtıktan sonra arşivi sil", "Arşiv çöp kutusuna gider", "extract_delete")}{/if}
          {@render sw("Sağlamayı kendiliğinden kontrol et", "Sunucuda dosya.sha256 gibi bir sağlama varsa biten dosyayla karşılaştırır", "auto_hash")}
          {@render sw("Bitince aç", "Biten dosyayı, arşivse açılan klasörü açar", "open_done")}
          <div class="line col">
            <div class="lh"><b>{tr("Bitince çalıştır")}</b></div>
            <input class="field mono" spellcheck="false" placeholder={tr(CMD_EX)} value={s.run_cmd} onchange={(e) => set({ run_cmd: e.currentTarget.value.trim() })} aria-label={tr("Bitince çalıştırılacak komut")} />
            <small><span class="mono">{"{path}"}</span>, <span class="mono">{"{folder}"}</span> {tr("ve")} <span class="mono">{"{name}"}</span> {tr("tırnaklı olarak yerine konur. Boşsa bir şey çalışmaz.")}</small>
          </div>
          <div class="line">
            <div><b>{tr("Hepsi bitince")}</b><small>{tr("Bir kez çalışır, sonra kendini kapatır")}</small></div>
            <div class="seg">{#each [["none", "Hiçbiri"], ["sleep", "Uyut"], ["hibernate", "Beklet"], ["shutdown", "güç|Kapat"], ["quit", "Çık"]] as [v, l]}<button class:on={s.after_all === v} onclick={() => set({ after_all: v })}>{tr(l)}</button>{/each}</div>
          </div>
        </div>
      {:else if cur === "indirme"}
        <div class="group">
          <div class="line col">
            <div class="lh"><b>{tr("Dosya başına bağlantı")}</b><span class="num val">{s.connections}</span></div>
            <input type="range" min="1" max="32" value={s.connections} onchange={(e) => set({ connections: +e.currentTarget.value })} aria-label={tr("Dosya başına bağlantı")} />
            <small>{tr("8 çoğu sunucu için en iyisi. Sunucu yoğunum derse Trawl sayıyı kendiliğinden düşürür.")}</small>
          </div>
          <div class="line col">
            <div class="lh"><b>{tr("Aynı anda indirme")}</b><span class="num val">{s.concurrent}</span></div>
            <input type="range" min="1" max="10" value={s.concurrent} onchange={(e) => set({ concurrent: +e.currentTarget.value })} aria-label={tr("Aynı anda indirme")} />
          </div>
          <div class="line col">
            <div class="lh"><b>{tr("Hız sınırı")}</b><span class="num val">{limitLabel}</span></div>
            <input type="range" min="0" max={LIMITS.length - 1} value={Math.max(0, LIMITS.indexOf(s.speed_limit))} oninput={(e) => set({ speed_limit: LIMITS[+e.currentTarget.value] })} aria-label={tr("Hız sınırı")} />
            <small>{tr("Toplam hıza uygulanır, anında geçerli olur.")}</small>
          </div>
          <div class="line col">
            <div class="lh"><b>{tr("Sunucu başına toplam bağlantı")}</b><span class="num val">{s.host_conns || tr("Sınırsız")}</span></div>
            <input type="range" min="0" max={HOSTC.length - 1} value={Math.max(0, HOSTC.indexOf(s.host_conns))} onchange={(e) => set({ host_conns: HOSTC[+e.currentTarget.value] })} aria-label={tr("Sunucu başına toplam bağlantı")} />
            <small>{tr("Aynı siteden birkaç dosya inerken hepsinin toplamı. Fazlası sitenin seni engellemesine yol açabilir.")}</small>
          </div>
        </div>
        <div class="group">
          {@render sw("Saate göre hız sınırı", "Belirlenen saatlerde genel sınırın yerine bu sınır geçerli olur, örneğin mesai saatinde", "alt_limit")}
          {#if s.alt_limit}
            <div class="line">
              <div><b>{tr("Saatler")}</b><small>{tr("Gece yarısını aşabilir")}</small></div>
              <div class="rowb">
                <input class="field num" type="time" value={s.alt_start} onchange={(e) => set({ alt_start: e.currentTarget.value })} aria-label={tr("Sınırın başladığı saat")} />
                <span class="dim">{tr("ile")}</span>
                <input class="field num" type="time" value={s.alt_stop} onchange={(e) => set({ alt_stop: e.currentTarget.value })} aria-label={tr("Sınırın bittiği saat")} />
              </div>
            </div>
            <div class="line col">
              <div class="lh"><b>{tr("Bu saatlerde")}</b><span class="num val">{kb(s.alt_speed)}</span></div>
              <input type="range" min="0" max={LIMITS.length - 1} value={Math.max(0, LIMITS.indexOf(s.alt_speed))} oninput={(e) => set({ alt_speed: LIMITS[+e.currentTarget.value] })} aria-label={tr("Saat aralığındaki hız sınırı")} />
            </div>
          {/if}
        </div>
        <div class="group">
          {@render sw("Zamanlayıcı", "Seçili günlerde belirlenen saatte sıradakileri başlatır, bitiş saatinde onları duraklatır", "schedule")}
          {#if s.schedule}
            <div class="line">
              <div><b>{tr("Saatler")}</b><small>{tr("Bilgisayar açık ve Trawl çalışıyor olmalı")}</small></div>
              <div class="rowb">
                <input class="field num" type="time" value={s.schedule_start} onchange={(e) => set({ schedule_start: e.currentTarget.value })} aria-label={tr("Başlangıç saati")} />
                <span class="dim">{tr("ile")}</span>
                <input class="field num" type="time" value={s.schedule_stop} onchange={(e) => set({ schedule_stop: e.currentTarget.value })} aria-label={tr("Bitiş saati")} />
              </div>
            </div>
            <div class="line">
              <div><b>{tr("Günler")}</b><small>{tr("Gece yarısını aşan aralık başladığı güne sayılır")}</small></div>
              <div class="seg">{#each ["Pzt", "Sal", "Çar", "Per", "Cum", "Cmt", "Paz"] as l, i}<button class:on={s.schedule_days?.includes(i)} aria-pressed={s.schedule_days?.includes(i)} onclick={() => set({ schedule_days: s.schedule_days?.includes(i) ? s.schedule_days.filter((d) => d !== i) : [...(s.schedule_days || []), i].sort() })}>{tr(l)}</button>{/each}</div>
            </div>
          {/if}
        </div>
        <div class="group">
          <div class="line col">
            <div class="lh"><b>Proxy</b></div>
            <input class="field mono" spellcheck="false" placeholder={tr("Boşsa sistem ayarı. Örnek: socks5://127.0.0.1:1080")} value={s.proxy} onchange={(e) => set({ proxy: e.currentTarget.value.trim() })} aria-label="Proxy" />
            <small>{tr("http, https ve socks5 desteklenir. Video indirmeleri de bunu kullanır.")}</small>
          </div>
        </div>
      {:else if cur === "medya"}
        <div class="group">
          <div class="line"><div><b>{tr("Video kalitesi")}</b><small>{tr("Yoksa bir alttaki seçilir")}</small></div>
            <div class="seg">{#each [2160, 1440, 1080, 720, 480] as h}<button class="num" class:on={s.default_height === h} onclick={() => set({ default_height: h })}>{h}p</button>{/each}</div></div>
          <div class="line"><div><b>{tr("Kapsayıcı")}</b><small>{tr("MP4 her oynatıcıda açılır")}</small></div>
            <div class="seg">{#each [["mp4", "MP4"], ["mkv", "MKV"], ["webm", "WebM"]] as [c, l]}<button class:on={s.default_container === c} onclick={() => set({ default_container: c })}>{l}</button>{/each}</div></div>
          <div class="line"><div><b>{tr("Ses biçimi")}</b></div>
            <div class="seg">{#each ["mp3", "m4a", "opus", "flac"] as c}<button class:on={s.default_codec === c} onclick={() => set({ default_codec: c })}>{c.toUpperCase()}</button>{/each}</div></div>
          <div class="line"><div><b>{tr("Ses bit hızı")}</b></div>
            <div class="seg">{#each [320, 256, 192, 128] as b}<button class="num" class:on={s.default_bitrate === b} onclick={() => set({ default_bitrate: b })}>{b} kbps</button>{/each}</div></div>
        </div>
        <div class="group">
          <div class="line"><div><b>{tr("Tarayıcının çerezleri")}</b><small>{tr("Yaş sınırlı ve üyelik isteyen videolar için oturumunu kullanır. Chrome ve Edge açıkken çerezlerini kilitler, en sorunsuzu Firefox")}</small></div>
            <div class="seg">{#each [["", "Kapalı"], ["firefox", "Firefox"], ["chrome", "Chrome"], ["edge", "Edge"], ["brave", "Brave"]] as [v, l]}<button class:on={(s.cookies_from || "") === v} onclick={() => set({ cookies_from: v })}>{tr(l)}</button>{/each}</div></div>
        </div>
      {:else if cur === "gorunum"}
        <div class="group">
          <div class="line"><div><b>{tr("Tema")}</b><small>{tr("Sistem, Windows ayarını izler")}</small></div>
            <div class="seg">{#each [["system", "Sistem"], ["light", "Açık"], ["dark", "Koyu"]] as [v, l]}<button class:on={s.theme === v} onclick={() => set({ theme: v })}>{tr(l)}</button>{/each}</div></div>
          <div class="line"><div><b>{tr("Vurgu rengi")}</b><small>{tr("Etkin indirme, seçim ve ana düğme")}</small></div>
            <div class="swatches" role="radiogroup" aria-label={tr("Vurgu rengi")}>
              {#each [["cobalt", "oklch(0.53 0.19 258)", "Kobalt"], ["signal", "oklch(0.565 0.2 40)", "Sinyal"], ["moss", "oklch(0.5 0.13 140)", "Yosun"]] as [v, c, l]}
                <button role="radio" aria-checked={s.accent === v} class:on={s.accent === v} onclick={() => set({ accent: v })} aria-label={tr(l)} data-tip={tr(l)}><i style="background:{c}"></i></button>
              {/each}
            </div></div>
          <div class="line"><div><b>{tr("Satır yüksekliği")}</b></div>
            <div class="seg">{#each [["compact", "Sıkı"], ["default", "Normal"], ["comfortable", "Rahat"]] as [v, l]}<button class:on={(s.density || "default") === v} onclick={() => set({ density: v })}>{tr(l)}</button>{/each}</div></div>
          <div class="line"><div><b>{tr("Hareket")}</b><small>{tr("Sistem, Windows'taki animasyon ayarını izler")}</small></div>
            <div class="seg">{#each [["system", "Sistem"], ["full", "Tam"], ["reduced", "Azaltılmış"], ["off", "Kapalı"]] as [v, l]}<button class:on={(s.motion_mode || "system") === v} onclick={() => set({ motion_mode: v })}>{tr(l)}</button>{/each}</div></div>
          <div class="line"><div><b>Dil / Language</b></div>
            <div class="seg">{#each [["tr", "Türkçe"], ["en", "English"]] as [v, l]}<button class:on={(s.lang || "tr") === v} onclick={() => set({ lang: v })}>{l}</button>{/each}</div></div>
        </div>
      {:else if cur === "tarayici"}
        <div class="group">
          {@render sw("İndirmeleri devral", "Chrome'da başlayan indirmeler Trawl'a gelir", "intercept")}
          <div class="line col">
            <div class="lh"><b>{tr("Devralma alt sınırı")}</b><span class="num val">{bytes(s.min_intercept_kb * 1024)}</span></div>
            <input type="range" min="0" max="10240" step="128" value={s.min_intercept_kb} onchange={(e) => set({ min_intercept_kb: +e.currentTarget.value })} aria-label={tr("Devralma alt sınırı")} />
            <small>{tr("Bundan küçük dosyalar Chrome'da kalır.")}</small>
          </div>
          <div class="line col">
            <div class="lh"><b>{tr("Önce sor")}</b><span class="num val">{s.intercept_ask_mb ? tr("{n} MB üstü", { n: s.intercept_ask_mb }) : tr("Hiç sorma")}</span></div>
            <input type="range" min="0" max={ASK.length - 1} value={Math.max(0, ASK.indexOf(s.intercept_ask_mb))} onchange={(e) => set({ intercept_ask_mb: ASK[+e.currentTarget.value] })} aria-label={tr("Önce sor")} />
            <small>{tr("Büyük indirmelerde klasör ve adı seçebileceğin pencere açılır. Küçükler hemen başlar.")}</small>
          </div>
          <div class="line col">
            <div class="lh"><b>{tr("Devralınacak türler")}</b><button class="link" onclick={() => set({ intercept_exts: DEFAULT_EXTS })}>{tr("Varsayılana dön")}</button></div>
            <textarea class="exts mono" rows="3" spellcheck="false" value={s.intercept_exts} onchange={(e) => set({ intercept_exts: e.currentTarget.value.replace(/[,.;]+/g, " ").replace(/\s+/g, " ").trim().toLowerCase() })} aria-label={tr("Devralınacak türler")}></textarea>
            <small>{tr("Uzantıları boşlukla ayır. Boş bırakırsan her indirme devralınır.")}</small>
          </div>
          <div class="line col">
            <div><b>{tr("Devralınmayan siteler")}</b><small>{tr("Eklenti menüsündeki \"Bu sitede devralma\" anahtarıyla eklenir.")}</small></div>
            {#if s.intercept_skip?.length}
              <div class="chips">{#each s.intercept_skip as h}<span class="chip mono">{h}<button aria-label={tr("{h} sitesini kaldır", { h })} onclick={() => set({ intercept_skip: s.intercept_skip.filter((x) => x !== h) })}><Icon n="x" size={12} /></button></span>{/each}</div>
            {/if}
          </div>
        </div>
        <p class="note">{tr("İpucu: Bir bağlantıya")} <span class="mono">Alt</span> {tr("basılıyken tıklarsan, kurallar ne derse desin Trawl'a gider.")}</p>
        <button class="btn" onclick={() => (store.modal = { type: "ext" })}><Icon n="puzzle" size={14} />{tr("Eklentiyi kur")}</button>
      {:else if cur === "kurallar"}
        <div class="group">
          <div class="line col">
            <div><b>{tr("Klasör kuralları")}</b><small>{tr("Uzantıya ya da siteye göre klasör seç. Yukarıdan aşağı ilk tutan kural geçerli, hiçbiri tutmazsa türüne göre klasör kullanılır.")}</small></div>
            {#each s.rules || [] as r, i}
              <div class="rule">
                <input class="field mono" spellcheck="false" placeholder="zip rar" value={r.exts} onchange={(e) => setRule(i, { exts: e.currentTarget.value.trim().toLowerCase() })} aria-label={tr("Uzantılar")} />
                <input class="field mono" spellcheck="false" placeholder="github.com" value={r.host} onchange={(e) => setRule(i, { host: e.currentTarget.value.trim().toLowerCase() })} aria-label="Site" />
                <button class="folder mono ellip" onclick={() => ruleFolder(i)} data-tip={tr("Klasör seç")}>{r.folder || tr("Klasör seç")}</button>
                <button class="icon-btn" aria-label={tr("Kuralı sil")} onclick={() => set({ rules: s.rules.filter((_, j) => j !== i) })}><Icon n="x" size={14} /></button>
              </div>
            {/each}
            <div><button class="btn btn-sm" onclick={() => set({ rules: [...(s.rules || []), { exts: "", host: "", folder: "" }] })}><Icon n="plus" size={14} />{tr("Kural ekle")}</button></div>
          </div>
        </div>
        <div class="group">
          <div class="line col">
            <div><b>{tr("Site girişleri")}</b><small>{tr("Kullanıcı adı ve şifre isteyen sunucular için (HTTP Basic). Video sitelerinde yt-dlp'ye verilir. Şifreler bu bilgisayardaki Windows hesabına bağlı şifrelenip saklanır.")}</small></div>
            {#each s.logins || [] as l, i}
              <div class="rule">
                <input class="field mono" spellcheck="false" placeholder={tr("ornek.com")} value={l.host} onchange={(e) => setLogin(i, { host: e.currentTarget.value.trim().toLowerCase() })} aria-label="Site" />
                <input class="field" spellcheck="false" autocomplete="off" placeholder={tr("Kullanıcı adı")} value={l.user} onchange={(e) => setLogin(i, { user: e.currentTarget.value })} aria-label={tr("Kullanıcı adı")} />
                <input class="field" type="password" autocomplete="new-password" placeholder={tr("Şifre")} value={l.pass} onchange={(e) => setLogin(i, { pass: e.currentTarget.value })} aria-label={tr("Şifre")} />
                <button class="icon-btn" aria-label={tr("Girişi sil")} onclick={() => set({ logins: s.logins.filter((_, j) => j !== i) })}><Icon n="x" size={14} /></button>
              </div>
            {/each}
            <div><button class="btn btn-sm" onclick={() => set({ logins: [...(s.logins || []), { host: "", user: "", pass: "" }] })}><Icon n="plus" size={14} />{tr("Giriş ekle")}</button></div>
          </div>
        </div>
      {:else if cur === "bilesen"}
        <p class="note">{tr("Uygulama klasörüne kurulur")} (<span class="mono">%APPDATA%\Trawl\bin</span>), {tr("sistemine dokunmaz. PATH'te varsa onlar kullanılır.")}</p>
        <div class="group">
          {#each TOOLS as t}
            {@const p = prog[t.n]}
            <div class="line">
              <div>
                <b>{t.n} {#if t.path}<span class="badge ok">{tr("Kurulu")}{t.ver ? ` · ${t.ver}` : ""}</span>{:else}<span class="badge">{tr("Yok")}</span>{/if}</b>
                <small class:mono={!!t.path} class="ellip">{t.path || tr(t.d)}</small>
                {#if busy && p}<div class="tp"><i style="transform:scaleX({p.total ? p.done / p.total : 1})"></i></div><small class="num">{p.stage ? tr(p.stage) : `${bytes(p.done)} / ${bytes(p.total)}`}</small>{/if}
              </div>
              <button class="btn btn-sm" disabled={!!busy} onclick={() => install(t.k)}>{#if busy === t.k}<span class="spinner"></span>{/if}{tr(t.path ? "Güncelle" : "Kur")}</button>
            </div>
          {/each}
        </div>
        <div class="group">
          {@render sw("yt-dlp'yi kendiliğinden güncelle", "Eksikleri kendisi kurar. yt-dlp'ye günde, deno'ya haftada bir bakar; ffmpeg'e dokunmaz. Yeni sürüm açılmazsa eskisine döner", "tools_auto_update")}
        </div>
        <button class="btn btn-primary" disabled={!!busy} onclick={() => install("all")}>{#if busy === "all"}<span class="spinner"></span>{/if}{tr("Hepsini kur ya da güncelle")}</button>
      {:else}
        <div class="about">
          <svg class="mark" viewBox="0 0 20 20" aria-hidden="true"><rect width="20" height="20" rx="4.5" fill="var(--text)" /><path d="M3.8 4.4h12.4v2.6h-4.7l-.75 5.6h-1.5l-.75-5.6H3.8z" fill="var(--bg)" /><circle cx="10" cy="14.35" r="2.15" fill="var(--accent)" /></svg>
          <div><b>Trawl</b><small class="num">{tr("Sürüm {v}", { v: "1.0.0" })}</small></div>
        </div>
        <dl>
          <dt>{tr("Motor")}</dt><dd>{tr("Rust; dosya başına 32'ye kadar bağlantı, dinamik parça bölme")}</dd>
          <dt>{tr("Devam")}</dt><dd>{tr("Parça durumu üç saniyede bir diske yazılır")}</dd>
          <dt>Video</dt><dd>{tr("yt-dlp, HLS ve DASH akışları, altyazı, bölümler")}</dd>
          <dt>{tr("Veri")}</dt><dd class="mono">%APPDATA%\Trawl</dd>
          <dt>{tr("Lisanslar")}</dt><dd>{tr("yt-dlp (Unlicense), FFmpeg (LGPL 2.1), deno (MIT), Mona Sans ve JetBrains Mono (OFL), Phosphor (MIT)")}</dd>
        </dl>
      {/if}
    </div>
  </section>
</div>

<style>
  .st { display: flex; width: 760px; height: 540px; max-height: calc(100vh - 64px); max-width: 100%; }
  aside { width: 196px; flex: none; display: flex; flex-direction: column; gap: 8px; padding: 12px 8px; background: var(--bg-sunken); border-right: 1px solid var(--border-subtle); }
  .search { display: flex; align-items: center; gap: 6px; height: 30px; padding: 0 8px; border-radius: var(--r-2); background: var(--bg); border: 1px solid var(--border-subtle); color: var(--text-3); margin-bottom: 4px; }
  .search:focus-within { border-color: var(--accent); }
  .search input { flex: 1; min-width: 0; border: 0; background: none; outline: none; color: var(--text); font-size: 12.5px; }
  nav { display: flex; flex-direction: column; gap: 1px; flex: 1; }
  nav button { height: 30px; padding: 0 10px; border-radius: var(--r-1); color: var(--text-2); text-align: left; font-weight: 500; }
  nav button:hover { background: var(--hover); color: var(--text); }
  nav button.on { background: var(--surface); color: var(--text); box-shadow: 0 0 0 1px var(--border-subtle); }
  .saved { height: 18px; font-size: 12px; color: var(--ok); padding: 0 10px; }
  .saved span { display: inline-flex; align-items: center; gap: 4px; animation: fade-in 150ms var(--ease-out); }
  section { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  header { display: flex; align-items: center; justify-content: space-between; padding: 14px 12px 10px 20px; }
  h2 { font-size: 16px; font-weight: 600; }
  .pane { flex: 1; overflow-y: auto; padding: 4px 20px 20px; display: flex; flex-direction: column; gap: 12px; }
  .pane > .btn { align-self: flex-start; }
  .note { color: var(--text-2); font-size: 12.5px; }
  .group { border: 1px solid var(--border-subtle); border-radius: var(--r-3); }
  .line { width: 100%; display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 10px 14px; min-height: 52px; text-align: left; border-bottom: 1px solid var(--border-subtle); }
  .line:last-child { border-bottom: 0; }
  button.line:hover { background: var(--hover); }
  .line > div:first-child { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .line b { font-weight: 500; display: flex; align-items: center; gap: 6px; }
  .line small { font-size: 12px; color: var(--text-3); }
  .line.col { flex-direction: column; align-items: stretch; gap: 8px; }
  .lh { display: flex !important; flex-direction: row !important; justify-content: space-between; align-items: baseline; }
  .val { font-weight: 600; }
  .rowb { display: flex; gap: 6px; flex: none; }
  .swatches { display: flex; gap: 6px; }
  .swatches button { width: 28px; height: 28px; border-radius: var(--r-2); display: grid; place-items: center; border: 1px solid var(--border); }
  .swatches button.on { border-color: var(--text); box-shadow: inset 0 0 0 1px var(--text); }
  .swatches i { width: 16px; height: 16px; border-radius: 3px; }
  .tp { height: 4px; margin-top: 6px; border-radius: 2px; background: var(--track); overflow: hidden; }
  .tp i { display: block; height: 100%; background: var(--accent); transform-origin: left; transition: transform 200ms linear; }
  .about { display: flex; align-items: center; gap: 12px; }
  .about .mark { width: 36px; height: 36px; }
  .about div { display: flex; flex-direction: column; }
  .about b { font-size: 15px; font-weight: 600; }
  .about small { color: var(--text-3); }
  dl { display: grid; grid-template-columns: 80px 1fr; gap: 8px 12px; font-size: 12.5px; padding-top: 12px; border-top: 1px solid var(--border-subtle); }
  dt { color: var(--text-3); }
  .exts { width: 100%; resize: vertical; padding: 8px 10px; border-radius: var(--r-2); border: 1px solid var(--border); background: var(--bg); color: var(--text); font-size: 12px; line-height: 1.5; outline: none; }
  .exts:focus { border-color: var(--accent); }
  .field { height: 30px; padding: 0 10px; border-radius: var(--r-2); border: 1px solid var(--border); background: var(--bg); color: var(--text); font-size: 12.5px; outline: none; }
  .line.col .field { width: 100%; }
  .field:focus { border-color: var(--accent); }
  .dim { color: var(--text-3); font-size: 12px; }
  .link { color: var(--accent-text); font-size: 12px; font-weight: 500; }
  .link:hover { text-decoration: underline; }
  .chips { display: flex; flex-wrap: wrap; gap: 6px; }
  .chip { display: inline-flex; align-items: center; gap: 4px; height: 24px; padding: 0 4px 0 8px; border-radius: var(--r-1); background: var(--surface-2); font-size: 12px; color: var(--text-2); }
  .chip button { width: 18px; height: 18px; display: grid; place-items: center; border-radius: 3px; color: var(--text-3); }
  .chip button:hover { background: var(--hover); color: var(--text); }
  dd { color: var(--text-2); }
  .rule { display: grid; grid-template-columns: 1fr 1fr 1.4fr auto; gap: 6px; align-items: center; }
  .rule .folder { height: 30px; padding: 0 8px; border: 1px dashed var(--border); border-radius: 6px; background: transparent; color: var(--text-2); font-size: 12px; text-align: left; }
</style>
