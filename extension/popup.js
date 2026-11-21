const $ = (s) => document.querySelector(s);
const fmt = (n) => { if (!n) return ""; const u = ["B", "KB", "MB", "GB", "TB"]; let i = 0; while (n >= 1024 && i < 4) { n /= 1024; i++; } return n.toFixed(i ? 1 : 0).replace(".", ",") + " " + u[i]; };
const dur = (s) => { if (!s) return ""; s = Math.round(s); const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60); return h ? `${h} sa ${m} dk` : m ? `${m} dk ${s % 60} sn` : `${s} sn`; };

function describe(it) {
  const host = (() => { try { return new URL(it.url).hostname; } catch { return ""; } })();
  const i = it.info;
  if (it.kind === "stream" && i) {
    if (i.drm) return { label: i.kind === "dash" ? "DASH" : "HLS", text: "Korumalı (DRM), indirilemez", drm: true };
    const parts = [i.height ? i.height + "p" : "", i.live ? "canlı" : dur(i.duration), i.size ? "~" + fmt(i.size) : "", i.variants.length > 1 ? i.variants.length + " kalite" : ""];
    return { label: i.kind === "dash" ? "DASH" : "HLS", text: parts.filter(Boolean).join(" · ") || host };
  }
  const label = it.kind === "stream" ? (/\.mpd/i.test(it.url) ? "DASH" : "HLS") : (it.ext || it.kind).slice(0, 4).toUpperCase();
  return { label, text: [fmt(it.size), host].filter(Boolean).join(" · ") };
}

(async () => {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  const s = await chrome.runtime.sendMessage({ type: "state", tabId: tab.id });
  const st = $("#st");
  st.classList.toggle("on", s.ok);
  st.querySelector("span").textContent = s.ok ? "bağlı" : "kapalı";
  $("#off").hidden = s.ok;
  // genel anahtar, site gizliyse onu da geri açıyor
  const pc = await chrome.storage.local.get(["panelOff", "panelHide"]);
  const ph = (pc.panelHide || []).filter((h) => s.host && (s.host === h || s.host.endsWith("." + h)));
  $("#panel").checked = !pc.panelOff && !ph.length;
  $("#phost").textContent = ph.length ? "bu sitede gizli" : "";
  $("#panel").onchange = async (e) => {
    if (e.target.checked) await chrome.storage.local.set({ panelOff: false, panelHide: (pc.panelHide || []).filter((h) => !ph.includes(h)) });
    else await chrome.storage.local.set({ panelOff: true });
    $("#phost").textContent = "";
  };
  if (s.ok && s.host) {
    $("#site").hidden = false;
    $("#host").textContent = s.host;
    $("#skip").checked = s.skip;
    $("#skip").onchange = (e) => chrome.runtime.sendMessage({ type: "site", host: s.host, skip: e.target.checked });
  }
  // akışlar önce, sonra büyükten küçüğe
  const items = s.items.sort((a, b) => (b.kind === "stream") - (a.kind === "stream") || (b.info?.size || b.size || 0) - (a.info?.size || a.size || 0));
  $("#cnt").textContent = items.length || "";
  const list = $("#list");
  if (!items.length) list.outerHTML = '<div class="empty">Henüz medya yakalanmadı. Videoyu oynat, burada belirsin.</div>';
  items.forEach((it, i) => {
    const li = document.createElement("li");
    li.style.animationDelay = i * 30 + "ms";
    const d = describe(it);
    li.innerHTML = `<span class="k ${it.kind}"></span><span class="n"><b></b><small></small></span><button class="dl" title="İndir"><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M12 4v12m0 0-5-5m5 5 5-5"/></svg></button>`;
    li.querySelector(".k").textContent = d.label;
    li.querySelector("b").textContent = it.kind === "stream" && tab.title ? tab.title : it.name;
    const sm = li.querySelector("small");
    sm.textContent = d.text;
    sm.classList.toggle("drm", !!d.drm);
    const btn = li.querySelector(".dl");
    if (d.drm) { btn.disabled = true; btn.title = "Bu video DRM ile korunuyor"; }
    btn.onclick = async () => {
      await chrome.runtime.sendMessage({ type: "send", tabId: tab.id, url: it.url, referer: tab.url, title: tab.title, mode: it.kind === "stream" ? "ask" : "direct" });
      li.style.opacity = ".4";
    };
    list.appendChild(li);
  });
  $("#page").onclick = async () => { await chrome.runtime.sendMessage({ type: "send", url: tab.url, referer: tab.url, title: tab.title }); window.close(); };
  $("#grab").onclick = async () => { await chrome.runtime.sendMessage({ type: "grab", tabId: tab.id }); window.close(); };
})();
