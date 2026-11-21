// MAIN dünyasında çalışıyor, videoya gerçekten DRM anahtarı bağlanınca işaretliyor.
// requestMediaKeySystemAccess'e güvenme, youtube onu sadece yetenek yoklamak için çağırıyor
(() => {
  const P = HTMLMediaElement.prototype;
  const orig = P.setMediaKeys;
  if (!orig || orig.__trawl) return;
  const wrapped = function (keys) {
    try { if (keys) this.setAttribute("data-trawl-drm", "1"); } catch {}
    return orig.apply(this, arguments);
  };
  wrapped.__trawl = true;
  P.setMediaKeys = wrapped;
})();

// bazı siteler m3u8/mpd'yi uzantısız bir api adresinden text/plain diye veriyor, ağ dinleyicisi bunu
// tanıyamıyor. içeriğin ilk baytlarına bakıp içerikten yakala
(() => {
  if (window.__trawlSniff) return;
  window.__trawlSniff = true;
  const seen = new Set();
  const skip = /\.(m3u8|mpd|js|mjs|css|png|jpe?g|gif|webp|svg|woff2?|ttf|ico|mp4|webm|ts|m4s|aac|mp3)(\?|#|$)/i;
  const report = (url, head) => {
    if (seen.has(url)) return;
    const kind = /^\s*#EXTM3U/.test(head) ? "hls" : /<MPD[\s>]/i.test(head) ? "dash" : "";
    if (!kind) return;
    seen.add(url);
    window.postMessage({ __trawl: "manifest", url, kind }, "*");
  };
  const worth = (url, type) => /^https?:/i.test(url) && !skip.test(url) && !/^(image|video|audio|font)\//i.test(type || "");
  const ofetch = window.fetch;
  if (ofetch) {
    window.fetch = function () {
      const p = ofetch.apply(this, arguments);
      p.then((r) => {
        try {
          if (!r.ok || !worth(r.url, r.headers.get("content-type")) || !r.body) return;
          const rd = r.clone().body.getReader();
          rd.read().then(({ value }) => { rd.cancel().catch(() => {}); if (value) report(r.url, new TextDecoder().decode(value.slice(0, 64))); }).catch(() => {});
        } catch {}
      }).catch(() => {});
      return p;
    };
  }
  const X = XMLHttpRequest.prototype;
  const osend = X.send;
  X.send = function () {
    this.addEventListener("load", () => {
      try {
        if (this.status >= 400 || !worth(this.responseURL, this.getResponseHeader("content-type"))) return;
        if (this.responseType === "" || this.responseType === "text") report(this.responseURL, String(this.responseText).slice(0, 64));
        else if (this.responseType === "arraybuffer" && this.response) report(this.responseURL, new TextDecoder().decode(new Uint8Array(this.response).slice(0, 64)));
      } catch {}
    });
    return osend.apply(this, arguments);
  };
})();
