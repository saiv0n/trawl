// her videonun üstüne İndir düğmesi ve kalite paneli. youtube'da oynatıcı altındaki düğme de duruyor
(() => {
  // eklenti yenilenince tekrar enjekte ediliyor, eskisi kendini kapatsın
  const ME = Symbol("trawl");
  window.__trawlPanel = ME;
  const alive = () => window.__trawlPanel === ME && !!chrome.runtime?.id;
  const HOST = location.hostname.replace(/^www\./, "").toLowerCase();
  const MIN_W = 200, MIN_H = 120, IDLE = 2500;
  const cfg = { off: false, hide: [] };
  const hiddenHere = () => cfg.off || cfg.hide.some((h) => HOST === h || HOST.endsWith("." + h));
  chrome.storage.local.get(["panelOff", "panelHide"]).then((s) => { cfg.off = !!s.panelOff; cfg.hide = s.panelHide || []; }).catch(() => {});
  chrome.storage.onChanged.addListener((c, area) => {
    if (area !== "local") return;
    if (c.panelOff) cfg.off = !!c.panelOff.newValue;
    if (c.panelHide) cfg.hide = c.panelHide.newValue || [];
    if (hiddenHere()) close(true);
  });

  const ICON = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v12m0 0-5-5m5 5 5-5M5 21h14"/></svg>';
  const CSS = `
    :host { all: initial; position: fixed; z-index: 2147483647; top: 0; left: 0; display: none; font: 13px/1.35 "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif;
      --bg: #f8f6f1; --sf1: #ece8e0; --line: #d8d3c8; --ink: #1a1814; --tx2: #6b665c; --tx3: #8c867c; --sig: #2f5ad6; --ok: #2d7a4b; --bad: #c2341d; --mono: "Cascadia Mono", Consolas, monospace; }
    @media (prefers-color-scheme: dark) { :host { --bg: #1c1a16; --sf1: #2a2722; --line: #36322b; --ink: #ede9e0; --tx2: #a29c90; --tx3: #7a746a; --sig: #7d9cf0; --ok: #5fb37e; --bad: #f06a52; } }
    * { box-sizing: border-box; margin: 0; }
    button { font: inherit; color: inherit; border: 0; background: none; cursor: pointer; }
    .b { display: inline-flex; align-items: center; gap: 6px; height: 30px; padding: 0 12px 0 10px; border-radius: 6px; background: #2f5ad6; color: #fff; font-weight: 600; font-size: 12.5px;
      box-shadow: 0 1px 2px rgb(0 0 0 / .25), 0 4px 12px rgb(0 0 0 / .18); opacity: 0; transform: translateY(-2px); transition: opacity 140ms ease-out, transform 140ms ease-out, filter 100ms; }
    :host(.on) .b { opacity: 1; transform: none; }
    .b:hover { filter: brightness(.9); } /* video üstünde: temadan bağımsız sabit kobalt (beyazla 5,9:1) */
    .b:focus-visible, .r:focus-visible, .x:focus-visible { outline: 2px solid var(--sig); outline-offset: 2px; }
    .m { position: absolute; top: 36px; right: 0; width: 290px; max-height: 360px; overflow: auto; background: var(--bg); color: var(--ink); border: 1px solid var(--line); border-radius: 8px;
      box-shadow: 0 8px 28px rgb(0 0 0 / .28); padding: 4px; display: none; }
    :host(.open) .m { display: block; animation: in 140ms ease-out; }
    @keyframes in { from { opacity: 0; transform: translateY(-4px); } }
    @media (prefers-reduced-motion: reduce) { .b, :host(.open) .m { transition: none; animation: none; } }
    .r { width: 100%; display: flex; align-items: center; gap: 10px; padding: 7px 8px; border-radius: 5px; text-align: left; }
    .r:hover, .r:focus-visible { background: var(--sf1); }
    .q { min-width: 46px; font: 700 11px var(--mono); color: var(--sig); }
    .t { flex: 1; min-width: 0; }
    .t b { display: block; font-weight: 560; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .t small { display: block; color: var(--tx3); font: 11px var(--mono); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .sep { height: 1px; background: var(--line); margin: 4px 2px; }
    .msg { padding: 10px; color: var(--tx2); }
    .msg.bad { color: var(--bad); }
    .msg.ok { color: var(--ok); font-weight: 600; }
    .x { width: 100%; padding: 6px 8px; border-radius: 5px; color: var(--tx3); font-size: 11.5px; text-align: left; }
    .x:hover { background: var(--sf1); color: var(--tx2); }
  `;

  let root, sh, btn, menu, cur = null, idleT = 0, raf = 0, open = false, over = false;
  const idle = () => {
    clearTimeout(idleT);
    idleT = setTimeout(() => { if (!open && !over) close(true); }, IDLE);
  };
  function ensure() {
    if (root?.isConnected) return;
    root = document.createElement("trawl-panel");
    sh = root.attachShadow({ mode: "closed" });
    sh.innerHTML = `<style>${CSS}</style><button class="b" aria-haspopup="menu" aria-expanded="false" title="Trawl ile indir">${ICON}<span>İndir</span></button><div class="m" role="menu"></div>`;
    btn = sh.querySelector(".b");
    menu = sh.querySelector(".m");
    btn.addEventListener("click", (e) => { e.preventDefault(); e.stopPropagation(); open ? close() : openMenu(); });
    // fare düğmede kıpırdamadan dursa da gizlenmesin
    root.addEventListener("mouseenter", () => { over = true; clearTimeout(idleT); });
    root.addEventListener("mouseleave", () => { over = false; idle(); });
    root.addEventListener("keydown", (e) => {
      if (e.key === "Escape") { close(); btn.focus(); }
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        const rows = [...sh.querySelectorAll(".r, .x")];
        const i = rows.indexOf(sh.activeElement);
        rows[(i + (e.key === "ArrowDown" ? 1 : rows.length - 1)) % rows.length]?.focus();
        e.preventDefault();
      }
    });
    (document.fullscreenElement || document.documentElement).appendChild(root);
  }

  function place() {
    if (!cur || !root) return;
    const r = cur.getBoundingClientRect();
    if (!cur.isConnected || r.width < MIN_W || r.height < MIN_H || r.bottom < 0 || r.top > innerHeight) return close(true);
    const w = btn.offsetWidth || 80;
    root.style.top = Math.max(6, r.top + 10) + "px";
    root.style.left = Math.min(innerWidth - w - 6, Math.max(6, r.right - w - 10)) + "px";
    raf = requestAnimationFrame(place);
  }

  function show(v) {
    ensure();
    if (cur !== v) { cur = v; if (open) close(); }
    root.style.display = "block";
    cancelAnimationFrame(raf);
    place();
    if (!root.classList.contains("on")) {
      void root.offsetWidth; // geçiş başlasın diye reflow. rAF arka plan sekmesinde kısılıyor
      root.classList.add("on");
    }
    idle();
  }

  function close(all = false) {
    open = false;
    if (!root) return;
    root.classList.remove("open");
    btn?.setAttribute("aria-expanded", "false");
    if (all) {
      root.classList.remove("on");
      root.style.display = "none";
      cancelAnimationFrame(raf);
      cur = null;
    } else {
      idle();
    }
  }

  function videoAt(x, y) {
    for (const v of document.querySelectorAll("video")) {
      const r = v.getBoundingClientRect();
      if (r.width >= MIN_W && r.height >= MIN_H && x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) return v;
    }
    return null;
  }

  let lastMove = 0;
  addEventListener("mousemove", (e) => {
    const t = performance.now();
    if (t - lastMove < 100) return;
    lastMove = t;
    if (!alive()) return close(true);
    if (open || hiddenHere()) return;
    if (document.fullscreenElement) return close(true);
    if (root && e.composedPath().includes(root)) return;
    const v = videoAt(e.clientX, e.clientY);
    if (v) show(v);
  }, { passive: true, capture: true });
  document.addEventListener("fullscreenchange", () => close(true));
  addEventListener("pointerdown", (e) => { if (open && !e.composedPath().includes(root)) close(); }, true);

  function el(tag, cls, text) {
    const n = document.createElement(tag);
    if (cls) n.className = cls;
    if (text != null) n.textContent = text;
    return n;
  }

  function message(text, kind = "") {
    menu.replaceChildren(el("div", "msg " + kind, text));
  }

  async function openMenu() {
    if (!cur) return;
    open = true;
    clearTimeout(idleT);
    root.classList.add("open");
    btn.setAttribute("aria-expanded", "true");
    message("Kaynaklar aranıyor…");
    const v = cur;
    const r = await chrome.runtime.sendMessage({ type: "sources", src: v.currentSrc || v.src || "", drm: v.getAttribute("data-trawl-drm") === "1", title: document.title }).catch(() => null);
    if (!open || cur !== v) return;
    if (!r) return message("Eklentiye ulaşılamadı. Sayfayı yenileyip yeniden dene.", "bad");
    if (!r.ok) return message("Trawl kapalı. Uygulamayı aç, sonra yeniden dene.", "bad");
    menu.replaceChildren();
    if (r.drm && !r.rows.some((x) => !x.page)) menu.append(el("div", "msg bad", "Bu video korumalı (DRM), indirilemez."));
    let pageDone = false;
    for (const row of r.rows) {
      if (row.page && !pageDone && menu.childElementCount) menu.append(el("div", "sep"));
      if (row.page) pageDone = true;
      if (row.page && r.drm && !r.rows.some((x) => !x.page)) continue;
      const b = el("button", "r");
      b.setAttribute("role", "menuitem");
      const t = el("span", "t");
      t.append(el("b", null, row.label), el("small", null, row.sub || ""));
      b.append(el("span", "q", row.q || "↓"), t);
      b.onclick = () => send(row);
      menu.append(b);
    }
    if (!r.drm && !r.rows.some((x) => !x.page)) {
      // sadece sayfa satırı var, kaynak ağda daha görünmedi
      const mse = /^blob:/.test(v.currentSrc || v.src || "");
      menu.prepend(el("div", "msg", mse
        ? "Bu video parça parça yükleniyor ve listesi henüz yakalanmadı. Videoyu başa sarıp oynat, sonra yeniden aç. Olmazsa sayfayı Trawl'da aç."
        : "Bu videonun kaynağı henüz görünmedi. Videoyu birkaç saniye oynat, sonra yeniden aç."));
    }
    menu.append(el("div", "sep"));
    const x = el("button", "x", "Bu sitede düğmeyi gösterme");
    x.setAttribute("role", "menuitem");
    x.onclick = async () => {
      const s = await chrome.storage.local.get("panelHide").catch(() => ({}));
      await chrome.storage.local.set({ panelHide: [...new Set([...(s.panelHide || []), HOST])] });
      close(true);
    };
    menu.append(x);
    sh.querySelector(".r, .x")?.focus();
  }

  async function send(row) {
    message("Gönderiliyor…");
    const r = await chrome.runtime.sendMessage({ type: "panelSend", row, title: document.title }).catch(() => null);
    if (!open) return;
    if (r?.ok) {
      message(row.mode === "ask" ? "Trawl'da açıldı" : "İndirme başladı", "ok");
      setTimeout(() => close(true), 1200);
    } else {
      message("Trawl kapalı. Uygulamayı aç, sonra yeniden dene.", "bad");
    }
  }
})();
