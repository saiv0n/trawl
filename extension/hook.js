// Alt+tık ile link kurala bakmadan Trawl'a gitsin, chrome'un kendi Alt+tık indirmesinin yerine
(() => {
  const ME = Symbol("trawl");
  window.__trawlHook = ME; // tekrar enjekte edilirse son gelen kazansın
  addEventListener(
    "click",
    (e) => {
      // sayfanın kendi ürettiği sahte tıklama sayılmasın
      if (!e.isTrusted || window.__trawlHook !== ME || !chrome.runtime?.id) return;
      if (!e.altKey || e.ctrlKey || e.metaKey || e.shiftKey || e.button !== 0) return;
      const a = e.composedPath().find((n) => n instanceof HTMLAnchorElement || n instanceof HTMLAreaElement);
      const href = a?.href;
      if (!href || !/^https?:/i.test(href)) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      chrome.runtime.sendMessage({ type: "alt", url: href, text: (a.textContent || "").trim().slice(0, 200) }).catch(() => {});
    },
    true
  );
})();

// main.js'in içerikten yakaladığı akışları arka plana ilet
addEventListener("message", (e) => {
  const d = e.data;
  if (e.source !== window || !d || d.__trawl !== "manifest" || typeof d.url !== "string" || !chrome.runtime?.id) return;
  chrome.runtime.sendMessage({ type: "manifest", url: d.url, kind: d.kind === "dash" ? "dash" : "hls" }).catch(() => {});
});
