// youtube'da beğen/paylaş satırına İndir düğmesi. kap çizilene kadar bekle, yüzen yedek yok
(() => {
  // yeniden enjekte edilince eski kopya kendini kapatıyor
  const ME = Symbol("trawl");
  window.__trawlYt = ME;
  const alive = () => window.__trawlYt === ME && !!chrome.runtime?.id;

  const ID = "trawl-yt-btn";
  const ICON = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v12m0 0-5-5m5 5 5-5M5 21h14"/></svg>';
  const HOSTS = ["ytd-watch-metadata #top-level-buttons-computed", "#above-the-fold #top-level-buttons-computed", "#actions-inner #menu"];

  function make() {
    const b = document.createElement("button");
    b.id = ID;
    b.innerHTML = ICON + "<span>İndir</span>";
    b.title = "Trawl ile indir · Shift: hemen video · Alt: hemen ses";
    b.onclick = async (e) => {
      e.preventDefault();
      e.stopPropagation();
      b.classList.add("busy");
      const title = document.title.replace(/ - YouTube$/, "");
      const mode = e.shiftKey ? "video" : e.altKey ? "audio" : "ask";
      const r = await chrome.runtime.sendMessage({ type: "yt", url: location.href, title, mode }).catch(() => ({ ok: false }));
      b.classList.remove("busy");
      b.classList.add(r?.ok ? "ok" : "bad");
      b.querySelector("span").textContent = r?.ok ? (mode === "ask" ? "Trawl'da" : "İndiriliyor") : "Uygulama kapalı";
      setTimeout(() => { b.classList.remove("ok", "bad"); b.querySelector("span").textContent = "İndir"; }, 2500);
    };
    return b;
  }

  function place() {
    const old = document.getElementById(ID);
    if (!alive()) return;
    const watch = location.pathname === "/watch" || location.pathname.startsWith("/shorts/");
    if (!watch) return old?.remove();
    const host = HOSTS.map((s) => document.querySelector(s)).find((h) => h && h.offsetParent !== null);
    if (!host) return; // daha çizilmedi, observer tekrar çağırır
    if (old?.parentElement === host) return;
    host.prepend(old || make());
  }

  // youtube saniyede yüzlerce DOM değişikliği yapıyor, kare başına bir kontrol yeter
  let queued = false;
  const schedule = () => {
    if (queued) return;
    queued = true;
    setTimeout(() => { queued = false; place(); }, 120);
  };
  const mo = new MutationObserver(() => (alive() ? schedule() : mo.disconnect()));
  mo.observe(document.documentElement, { childList: true, subtree: true });
  document.addEventListener("yt-navigate-finish", schedule);
  place();
})();
