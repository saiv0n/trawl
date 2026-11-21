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
