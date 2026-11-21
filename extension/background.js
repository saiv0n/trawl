// eklentinin arka planı, uygulamayla konuşan taraf
const APP = "http://127.0.0.1:47321";
const SKIP = /\.(ts|m4s|cmfv|cmfa)(\?|$)|segment|\/range\/|init\.mp4|\/frag\(|\/seg-?\d+/i;
const VIDEO_EXT = /^(mp4|m4v|webm|mkv|mov|flv|avi|wmv|3gp|ogv)$/i;
const AUDIO_EXT = /^(mp3|m4a|aac|flac|wav|ogg|opus|wma)$/i;
// uygulamaya geçecek başlıklar (çerez, referer, ua, sitelerin X-* şeyleri)
const KEEP_HDR = /^(user-agent|referer|origin|cookie|authorization|accept-language|x-[\w-]+)$/i;
const TYPES = ["media", "xmlhttprequest", "other", "object", "main_frame", "sub_frame"];
let lastPoll = 0;
let alive = { ok: false, at: 0, intercept: true, min_kb: 512, exts: [], skip: [], ask_mb: 0, known: false };
const NATIVE = "com.trawl.bridge";
const UA = navigator.userAgent;
const BROWSER = /Firefox\//.test(UA) ? "firefox" : /Edg\//.test(UA) ? "edge" : /OPR\//.test(UA) ? "opera" : /Vivaldi/.test(UA) ? "vivaldi" : navigator.brave ? "brave" : "chrome";
const VERSION = chrome.runtime.getManifest().version;
// uygulama kapalıyken de kuralları bilelim diye saklıyorum
chrome.storage.local.get("cfg").then((s) => { if (s.cfg && !alive.known) alive = { ...alive, ...s.cfg, ok: false, at: 0, known: true }; }).catch(() => {});

function adopt(j) {
  const cfg = { intercept: j.intercept, min_kb: j.min_kb ?? 512, proto: j.proto || 1, exts: j.exts || [], skip: j.skip || [], ask_mb: j.ask_mb || 0 };
  alive = { ...cfg, ok: j.app === "Trawl", at: Date.now(), known: true };
  chrome.storage.local.set({ cfg }).catch(() => {});
}

async function ping(force = false) {
  if (!force && Date.now() - alive.at < 4000) return alive.ok;
  try {
    const r = await fetch(`${APP}/ping?v=${VERSION}&b=${BROWSER}`, { cache: "no-store" });
    adopt(await r.json());
  } catch {
    alive = { ...alive, ok: false, at: Date.now() };
  }
  if (alive.ok && Date.now() - lastPoll > 10000) pollJobs();
  return alive.ok;
}

// native messaging, uygulama kapalıysa açtırıyor. kayıt yoksa null
async function native(path, body, launch = false) {
  try {
    return await chrome.runtime.sendNativeMessage(NATIVE, { path, body, launch });
  } catch {
    return null;
  }
}

// uygulama kapalıysa aç, ayağa kalkınca true
async function ensureApp() {
  if (await ping(true)) return true;
  const r = await native("/ping", null, true);
  if (r?.ok && r.data) {
    adopt(r.data);
    return ping(true).then((ok) => ok || alive.ok);
  }
  return false;
}

async function cookiesFor(url) {
  try {
    const list = await chrome.cookies.getAll({ url });
    return list.map((c) => `${c.name}=${c.value}`).join("; ");
  } catch {
    return "";
  }
}

async function send(path, body) {
  try {
    const r = await fetch(APP + path, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
    return r.ok ? await r.json().catch(() => ({ ok: true })) : null;
  } catch {
    // bazı antivirüsler localhost'u kesiyor, o zaman native'den git
    const n = await native(path, body);
    return n?.ok ? n.data || { ok: true } : null;
  }
}

// gerçek başlıklar yoksa çerez + referer ile idare et
async function sendUrl(url, { referer = "", mode = "ask", filename = "", title = "", headers = null, size = 0, height = 0 } = {}) {
  if (!(await ensureApp())) return false;
  const h = { ...(headers || {}) };
  if (!Object.keys(h).some((k) => /^cookie$/i.test(k))) {
    const c = await cookiesFor(url);
    if (c) h.Cookie = c;
  }
  if (referer && !Object.keys(h).some((k) => /^referer$/i.test(k))) h.Referer = referer;
  const r = await send("/add", { url, referer, headers: h, filename, mode, title, page: referer, size, height });
  return !!r;
}

// istek başlıkları, kısa süre tutuluyor
const reqHdr = new Map();
const urlHdr = new Map();
function cap(m, n) {
  while (m.size > n) m.delete(m.keys().next().value);
}
function onSendHeaders(d) {
  const h = {};
  for (const x of d.requestHeaders || []) if (x.value && (KEEP_HDR.test(x.name) || (d.method === "POST" && /^content-type$/i.test(x.name)))) h[x.name] = x.value;
  reqHdr.set(d.requestId, h);
  urlHdr.set(d.url, h);
  cap(reqHdr, 3000);
  cap(urlHdr, 600);
}
try {
  // chromium Cookie/Referer'ı extraHeaders olmadan göstermiyor
  chrome.webRequest.onSendHeaders.addListener(onSendHeaders, { urls: ["<all_urls>"], types: TYPES }, ["requestHeaders", "extraHeaders"]);
} catch {
  // firefox bunu tanımıyor ama zaten hepsini veriyor
  chrome.webRequest.onSendHeaders.addListener(onSendHeaders, { urls: ["<all_urls>"], types: TYPES }, ["requestHeaders"]);
}

// sekme başına yakalananlar, worker uyusa da dursun
const found = new Map(); // tabId -> Map(url -> item)
const loading = new Map(); // tabId -> Promise
function tabMap(tabId) {
  if (found.has(tabId)) return Promise.resolve(found.get(tabId));
  if (!loading.has(tabId)) {
    loading.set(
      tabId,
      chrome.storage.session
        .get("t" + tabId)
        .catch(() => ({}))
        .then((s) => {
          if (!found.has(tabId)) found.set(tabId, new Map((s["t" + tabId] || []).map((i) => [i.url, i])));
          loading.delete(tabId);
          return found.get(tabId);
        })
    );
  }
  return loading.get(tabId);
}
const dirty = new Set();
let saveT = 0;
function persist(tabId) {
  dirty.add(tabId);
  clearTimeout(saveT);
  saveT = setTimeout(async () => {
    const o = {};
    for (const id of dirty) o["t" + id] = [...(found.get(id)?.values() || [])].slice(-200);
    dirty.clear();
    await chrome.storage.session.set(o).catch(() => {});
  }, 300);
}
function clearTab(tabId) {
  found.set(tabId, new Map());
  chrome.storage.session.remove("t" + tabId).catch(() => {});
  badge(tabId);
}
const visible = (m) => [...(m?.values() || [])].filter((i) => !i.child);

function badge(tabId) {
  const n = visible(found.get(tabId)).length;
  chrome.action.setBadgeText({ tabId, text: n ? String(n > 99 ? "99+" : n) : "" }).catch(() => {});
  chrome.action.setBadgeBackgroundColor({ tabId, color: "#2F5AD6" }).catch(() => {});
}

function extOf(s) {
  return ((s || "").split(/[?#]/)[0].match(/\.(\w{2,5})$/) || [])[1]?.toLowerCase() || "";
}
function dispName(cd) {
  if (!cd) return "";
  let m = cd.match(/filename\*\s*=\s*[^']*''([^;]+)/i);
  if (m) try { return decodeURIComponent(m[1].replace(/"/g, "").trim()); } catch {}
  m = cd.match(/filename\s*=\s*"?([^";]+)"?/i);
  return m ? m[1].trim() : "";
}

// başlığa bak, uzantı sadece ipucu
function kindOf(url, mime, name) {
  mime = (mime || "").toLowerCase();
  const path = (() => { try { return new URL(url).pathname; } catch { return url; } })();
  if (/mpegurl/.test(mime) || /\.m3u8$/i.test(path)) return "stream";
  if (/dash\+xml/.test(mime) || /\.mpd$/i.test(path)) return "stream";
  if (mime === "video/mp2t") return "ts";
  if (mime.startsWith("video/")) return "video";
  if (mime.startsWith("audio/")) return "audio";
  const e = extOf(name) || extOf(path);
  const generic = !mime || /octet-stream|binary|force-download|x-download|application\/mp4/.test(mime);
  if (generic && VIDEO_EXT.test(e)) return "video";
  if (generic && AUDIO_EXT.test(e)) return "audio";
  if (mime === "application/mp4") return "video";
  return "";
}

// worker istekleri tabId -1 geliyor, origin'den sekmeyi bul
async function tabForInitiator(init) {
  if (!init || !/^https?:/.test(init)) return -1;
  const tabs = await chrome.tabs.query({}).catch(() => []);
  const same = tabs.filter((t) => { try { return new URL(t.url).origin === init; } catch { return false; } });
  if (!same.length) return -1;
  same.sort((a, b) => (b.active - a.active) || ((b.lastAccessed || 0) - (a.lastAccessed || 0)));
  return same[0].id;
}

function cleanUrl(u) {
  try {
    const x = new URL(u);
    ["range", "rn", "rbuf", "bytestart", "byteend"].forEach((p) => x.searchParams.delete(p));
    return x.toString();
  } catch {
    return u;
  }
}

chrome.webRequest.onHeadersReceived.addListener(
  (d) => {
    const req = reqHdr.get(d.requestId) || {};
    const rj = refreshing.get(d.tabId);
    if (rj && d.statusCode < 300) {
      // tazeleme sekmesindeyiz, boyutu tutan her şey aday
      const h = Object.fromEntries((d.responseHeaders || []).map((x) => [x.name.toLowerCase(), x.value || ""]));
      const total = h["content-range"]?.includes("/") ? parseInt(h["content-range"].split("/")[1]) : parseInt(h["content-length"] || "0");
      if (total === rj.job.size) rj.cands.set(d.url, req);
    }
    capture(d, req).catch(() => {});
  },
  { urls: ["<all_urls>"], types: TYPES },
  ["responseHeaders"]
);

async function capture(d, req) {
  if (d.statusCode >= 400 || (d.statusCode >= 300 && d.statusCode < 400)) return;
  const h = Object.fromEntries((d.responseHeaders || []).map((x) => [x.name.toLowerCase(), x.value || ""]));
  const mime = (h["content-type"] || "").split(";")[0].trim();
  const dname = dispName(h["content-disposition"]);
  let k = kindOf(d.url, mime, dname);
  if (!k) return;
  if (SKIP.test(d.url) && k !== "stream") return;
  let size = 0;
  const cr = h["content-range"];
  if (cr && cr.includes("/")) size = parseInt(cr.split("/")[1]) || 0;
  else size = parseInt(h["content-length"] || "0") || 0;
  if (k === "ts") {
    if (size < 5 << 20) return; // hls parçası, tek parça .ts video olsa büyük olurdu
    k = "video";
  }
  if (k !== "stream" && size && size < 150 * 1024) return; // ufak parçalar, arayüz sesleri vs
  let tabId = d.tabId;
  if (tabId < 0) tabId = await tabForInitiator(d.initiator);
  if (tabId < 0) return;
  const m = await tabMap(tabId);
  const clean = cleanUrl(d.url);
  const headers = { ...req };
  if (!Object.keys(headers).some((x) => /^referer$/i.test(x)) && d.initiator && /^https?:/.test(d.initiator)) headers.Referer = d.initiator + "/";
  const old = m.get(clean);
  if (!old) {
    let name = dname;
    if (!name) try { name = decodeURIComponent(new URL(clean).pathname.split("/").pop() || ""); } catch {}
    const it = { url: clean, kind: k, name: name || "medya", ext: extOf(name), size, mime, source: "ağ", thumb: "", title: "", page: "", frame: d.frameId || 0, headers, at: Date.now(), info: null, child: false };
    // çözülmüş bir master'ın varyantıysa gösterme
    if (k === "stream") for (const o of m.values()) if (o.info?.master && isChildOf(o.info, clean)) it.child = true;
    m.set(clean, it);
    badge(tabId);
    persist(tabId);
    if (k === "stream" && !it.child) probeStream(tabId, it);
  } else if (size > (old.size || 0)) {
    old.size = size;
    persist(tabId);
  }
}

// hls/dash çözümleme
function sameStream(a, b) {
  try {
    const x = new URL(a), y = new URL(b);
    return x.origin + x.pathname === y.origin + y.pathname;
  } catch {
    return a === b;
  }
}
const isChildOf = (info, url) => (info.kind === "hls") && [...info.variants.map((v) => v.url), ...info.audio].some((u) => sameStream(u, url));
const probing = new Set();
async function probeStream(tabId, it) {
  if (it.info || probing.has(it.url) || !(await ping())) return;
  probing.add(it.url);
  try {
    const r = await send("/probe-stream", { url: it.url, headers: it.headers || {} });
    if (!r?.ok) return;
    const m = await tabMap(tabId);
    const cur = m.get(it.url);
    if (!cur) return;
    cur.info = r.info;
    if (r.info.master) {
      for (const o of m.values()) if (o !== cur && o.kind === "stream" && isChildOf(r.info, o.url)) o.child = true;
    } else {
      // varyant master'dan önce gelebilir, master gelince gizleniyor. tersi de olur
      for (const o of m.values()) if (o !== cur && o.info?.master && isChildOf(o.info, cur.url)) cur.child = true;
    }
    badge(tabId);
    persist(tabId);
  } finally {
    probing.delete(it.url);
  }
}

chrome.tabs.onUpdated.addListener((tabId, info) => {
  if (info.status === "loading" && info.url) clearTab(tabId);
});
chrome.tabs.onRemoved.addListener((tabId) => {
  found.delete(tabId);
  chrome.storage.session.remove("t" + tabId).catch(() => {});
});
chrome.tabs.onActivated.addListener(({ tabId }) => tabMap(tabId).then(() => badge(tabId)));

// indirme devralma
// POST'la inen dosyaların form gövdesi, 60 sn tutuluyor
const postBody = new Map();
chrome.webRequest.onBeforeRequest.addListener(
  (d) => {
    if (d.method !== "POST" || !d.requestBody) return;
    let body = null, form = false;
    const rb = d.requestBody;
    if (rb.formData) {
      const p = new URLSearchParams();
      for (const [k, vs] of Object.entries(rb.formData)) for (const v of vs) p.append(k, v);
      body = p.toString();
      form = true;
    } else if (rb.raw?.length && rb.raw.every((r) => r.bytes)) {
      const parts = rb.raw.map((r) => new Uint8Array(r.bytes));
      const all = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
      let o = 0;
      for (const p of parts) { all.set(p, o); o += p.length; }
      try { body = new TextDecoder("utf-8", { fatal: true }).decode(all); } catch { body = null; } // ikiliyse taşıyamıyoruz
    }
    postBody.set(d.url, { body, form, at: Date.now() });
    cap(postBody, 100);
  },
  { urls: ["<all_urls>"], types: ["main_frame", "sub_frame", "other"] },
  ["requestBody"]
);

const hostOf = (u) => { try { return new URL(u).hostname.replace(/^www\./, "").toLowerCase(); } catch { return ""; } };
const onSite = (host, list) => !!host && list.some((s) => host === s || host.endsWith("." + s));
const extOfName = (n) => ((n || "").split(/[\\/]/).pop().match(/\.([a-z0-9]{1,8})$/i) || [])[1]?.toLowerCase() || "";

// chrome adı ve boyutu başlıklardan çıkardıktan sonra karar veriyoruz, o yüzden ikisi de kesin.
// uygulama almadan chrome'daki indirmeyi iptal etme
async function decide(item) {
  const url = item.finalUrl || item.url;
  if (!/^https?:/.test(url) || item.byExtensionId === chrome.runtime.id) return null;
  // uygulama kapalı, son kurallarla karar ver. uyuyorsa aşağıda açtırılıyor
  if (!(await ping()) && !alive.known) return null;
  if (!alive.intercept) return null;
  if (onSite(hostOf(item.referrer), alive.skip) || onSite(hostOf(url), alive.skip)) return null;
  const ext = extOfName(item.filename) || extOfName(new URL(url).pathname);
  if (alive.exts.length && !alive.exts.includes(ext)) return null;
  if (item.totalBytes > 0 && item.totalBytes < alive.min_kb * 1024) return null;
  const headers = { ...(urlHdr.get(url) || urlHdr.get(item.url) || {}) };
  const pb = postBody.get(item.url) || postBody.get(url);
  if (pb && Date.now() - pb.at < 60000) {
    if (pb.body == null) return null; // ikili POST, chrome indirsin
    headers[":body"] = pb.body;
    if (pb.form) {
      for (const k of Object.keys(headers)) if (/^content-type$/i.test(k)) delete headers[k];
      headers["Content-Type"] = "application/x-www-form-urlencoded";
    }
  }
  if (!alive.ok && !(await ensureApp())) return null;
  const post = ":body" in headers;
  const big = alive.ask_mb > 0 && item.totalBytes > alive.ask_mb * 1048576;
  return { url: post ? item.url : url, headers, mode: big && !post ? "ask" : "direct", size: item.totalBytes || 0 };
}

async function takeOver(item) {
  const rj = [...refreshing.values()].find((r) => sameFile(r.job, item));
  if (rj) {
    // tazeleme sekmesinde kendi başlayan indirme, yeni iş değil aday
    const u = item.finalUrl || item.url;
    rj.cands.set(u, urlHdr.get(u) || urlHdr.get(item.url) || {});
    await chrome.downloads.cancel(item.id).catch(() => {});
    await chrome.downloads.erase({ id: item.id }).catch(() => {});
    return;
  }
  const d = await decide(item);
  if (!d) return;
  const fname = (item.filename || "").split(/[\\/]/).pop();
  const ok = await sendUrl(d.url, { referer: item.referrer || "", mode: d.mode, filename: fname, headers: d.headers, size: d.size });
  if (!ok) return; // almadıysa tarayıcı devam etsin
  await chrome.downloads.cancel(item.id).catch(() => {});
  await chrome.downloads.erase({ id: item.id }).catch(() => {});
}

if (chrome.downloads.onDeterminingFilename) {
  // chromium, ad ve boyut belli ama dosya daha diske yazılmadı
  chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
    takeOver(item).catch(() => {}).finally(() => { try { suggest(); } catch {} });
    return true; // suggest sonra çağrılıyor
  });
} else {
  // firefox'ta bu olay yok, oluşur oluşmaz al
  chrome.downloads.onCreated.addListener((item) => { takeOver(item).catch(() => {}); });
}

// sağ tık
// kurulunca açık sekmelere de enjekte et, yoksa F5 lazım
async function injectOpenTabs() {
  const tabs = await chrome.tabs.query({ url: ["http://*/*", "https://*/*"] }).catch(() => []);
  for (const t of tabs) {
    const target = { tabId: t.id, allFrames: true };
    const yt = /^https:\/\/(www|m|music)\.youtube\.com\//.test(t.url || "");
    chrome.scripting.executeScript({ target, files: ["main.js"], world: "MAIN" }).catch(() => {});
    chrome.scripting.executeScript({ target, files: ["hook.js", "panel.js"] }).catch(() => {});
    if (yt) {
      chrome.scripting.insertCSS({ target: { tabId: t.id }, files: ["youtube.css"] }).catch(() => {});
      chrome.scripting.executeScript({ target: { tabId: t.id }, files: ["youtube.js"] }).catch(() => {});
    }
  }
}

chrome.runtime.onInstalled.addListener(() => {
  injectOpenTabs();
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({ id: "link", title: "Trawl ile indir", contexts: ["link", "video", "audio"] });
    chrome.contextMenus.create({ id: "img", title: "Görseli en büyük boyutuyla indir", contexts: ["image"] });
    chrome.contextMenus.create({ id: "sel", title: "Seçili bağlantıları Trawl ile indir", contexts: ["selection"] });
    chrome.contextMenus.create({ id: "links", title: "Bu sayfadaki bütün bağlantılar…", contexts: ["page", "frame"] });
    chrome.contextMenus.create({ id: "page", title: "Bu sayfadaki her şeyi Trawl'da tara", contexts: ["page", "frame"] });
  });
});

// seçicinin türleri, uygulamadaki scrape::kind_of_ext ile aynı tut
const KINDS = [
  ["video", /^(mp4|mkv|webm|avi|mov|wmv|flv|m4v|ts|3gp|mpg|mpeg|ogv)$/],
  ["audio", /^(mp3|m4a|aac|flac|wav|ogg|opus|wma|alac|aiff|mid)$/],
  ["doc", /^(pdf|docx?|xlsx?|pptx?|txt|epub|csv|odt|rtf|mobi|djvu)$/],
  ["app", /^(exe|msi|apk|dmg|deb|rpm|appimage|msix|jar|xapk)$/],
  ["archive", /^(zip|rar|7z|tar|gz|bz2|xz|iso|img|zst|tgz|cab)$/],
  ["image", /^(jpe?g|png|gif|webp|bmp|svg|tiff|avif|heic|ico|psd)$/],
  ["stream", /^(m3u8|mpd)$/],
];
function kindForUrl(u) {
  const e = extOf(u);
  const k = KINDS.find(([, re]) => re.test(e));
  if (k) return k[0];
  const h = hostOf(u);
  return MEDIA_HOSTS.some((m) => h === m || h.endsWith("." + m)) ? "embed" : "other";
}

// seçimdeki ya da sayfadaki linkler, uygulamada seçici açılıyor
async function grabLinks(tab, frameId, onlySelection) {
  if (!(await ensureApp())) return false;
  let r = null;
  try {
    [{ result: r }] = await chrome.scripting.executeScript({
      target: { tabId: tab.id, frameIds: [frameId || 0] },
      args: [onlySelection],
      func: (onlySel) => {
        const sel = getSelection();
        const ranges = onlySel && sel ? Array.from({ length: sel.rangeCount }, (_, i) => sel.getRangeAt(i)) : null;
        const out = [], seen = new Set();
        for (const a of document.querySelectorAll("a[href], area[href]")) {
          if (ranges && !ranges.some((rg) => rg.intersectsNode(a))) continue;
          if (!/^https?:/.test(a.href) || seen.has(a.href)) continue;
          seen.add(a.href);
          out.push({ url: a.href, text: (a.textContent || a.title || "").trim().replace(/\s+/g, " ").slice(0, 150) });
        }
        if (onlySel && !out.length && sel) for (const m of String(sel).matchAll(/https?:\/\/[^\s<>"'`]+/g)) out.push({ url: m[0].replace(/[),.;:!?\]]+$/, ""), text: "" });
        return { items: out.slice(0, 3000), url: location.href, title: document.title };
      },
    });
  } catch {}
  if (!r?.items?.length) return false;
  const items = r.items
    .map((l) => {
      const kind = kindForUrl(l.url);
      let name = "";
      try { name = decodeURIComponent(new URL(l.url).pathname.split("/").pop() || "") || hostOf(l.url); } catch {}
      return { url: l.url, kind, name, title: l.text, ext: extOf(l.url), size: 0, mime: "", source: onlySelection ? "seçim" : "bağlantı", thumb: "", page: r.url, headers: { Referer: r.url } };
    })
    // seçimde ne varsa al. tüm sayfada sadece dosya/medya, gezinme linkleri kalabalık yapıyor
    .filter((i) => onlySelection || i.kind !== "other");
  if (!items.length) return false;
  return !!(await send("/grab", { url: r.url, title: r.title || tab.title, icon: tab.favIconUrl || "", items }));
}

// srcset/picture'dan en büyüğü
async function largestImage(tab, frameId, src) {
  try {
    const [{ result }] = await chrome.scripting.executeScript({
      target: { tabId: tab.id, frameIds: [frameId || 0] },
      args: [src],
      func: (src) => {
        const img = [...document.images].find((i) => i.currentSrc === src || i.src === src);
        if (!img) return src;
        const sets = [img.srcset, ...[...(img.closest("picture")?.querySelectorAll("source") || [])].map((s) => s.srcset)].filter(Boolean);
        let best = src, bw = img.naturalWidth || 0;
        for (const set of sets)
          for (const part of set.split(/,\s+(?=\S)/)) {
            const [u, d = "1x"] = part.trim().split(/\s+/);
            const w = /w$/.test(d) ? parseFloat(d) : (parseFloat(d) || 1) * (img.naturalWidth || 1);
            if (w > bw) { bw = w; best = new URL(u, location.href).href; }
          }
        return best;
      },
    });
    return result || src;
  } catch {
    return src;
  }
}

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  const referer = info.frameUrl || tab?.url || "";
  if (info.menuItemId === "link") {
    const url = info.linkUrl || info.srcUrl;
    if (url) sendUrl(url, { referer, headers: urlHdr.get(url) || null });
  } else if (info.menuItemId === "img" && info.srcUrl) {
    const url = /^https?:/.test(info.srcUrl) ? await largestImage(tab, info.frameId, info.srcUrl) : "";
    if (url) sendUrl(url, { referer, mode: "direct", headers: urlHdr.get(url) || null });
  } else if ((info.menuItemId === "sel" || info.menuItemId === "links") && tab) {
    grabLinks(tab, info.frameId, info.menuItemId === "sel");
  } else if (info.menuItemId === "page" && tab) {
    grabTab(tab);
  }
});

// sayfa ve iframe'lerdeki medya (gömülü oynatıcılar), üstüne ağdan yakalananlar
async function grabTab(tab) {
  if (!(await ensureApp())) return false;
  let dom = [];
  try {
    const res = await chrome.scripting.executeScript({
      target: { tabId: tab.id, allFrames: true },
      func: () => {
        const out = [];
        const add = (url, kind) => url && /^https?:/.test(url) && out.push({ url, kind, title: document.title, frame: location.href });
        document.querySelectorAll("video, audio").forEach((m) => {
          const k = m.tagName === "AUDIO" ? "audio" : "video";
          add(m.currentSrc || m.src, k);
          m.querySelectorAll("source").forEach((s) => add(s.src, k));
        });
        return out;
      },
    });
    dom = res.flatMap((r) => r?.result || []);
  } catch {}
  const m = await tabMap(tab.id);
  const items = visible(m).map((i) => ({ ...i }));
  for (const d of dom) {
    if (items.some((i) => i.url === d.url)) continue;
    items.push({ url: d.url, kind: d.kind, title: d.title, name: d.url.split("/").pop().split("?")[0], ext: extOf(d.url), size: 0, mime: "", source: "dom", thumb: "", page: tab.url, headers: { ...(urlHdr.get(d.url) || {}), Referer: d.frame } });
  }
  for (const i of items) i.page = tab.url;
  return !!(await send("/grab", { url: tab.url, title: tab.title, icon: tab.favIconUrl || "", items }));
}

// bağlantı tazeleme
// link süresi dolunca uygulama iş yolluyor. kaynak sayfayı sessiz bir sekmede açıp adayları topluyoruz,
// uygulama boyutu tutanı seçip devam ediyor
const refreshing = new Map(); // tabId -> { job, cands: Map(url -> headers) }
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const tailOf = (u) => { try { return decodeURIComponent(new URL(u).pathname.split("/").pop() || "").toLowerCase(); } catch { return ""; } };
function sameFile(job, item) {
  const n = (item.filename || "").split(/[\\/]/).pop().toLowerCase();
  return (item.totalBytes > 0 && item.totalBytes === job.size) || (!!n && n === job.name.toLowerCase());
}

chrome.alarms.create("jobs", { periodInMinutes: 0.5 });
chrome.alarms.onAlarm.addListener((a) => { if (a.name === "jobs") pollJobs(); });

async function pollJobs() {
  lastPoll = Date.now();
  if (!(await ping())) return;
  const r = await send("/jobs", {});
  for (const j of r?.jobs || []) refreshJob(j).catch(() => {});
}

function waitLoad(tabId, ms) {
  return new Promise((res) => {
    const done = () => { chrome.tabs.onUpdated.removeListener(fn); clearTimeout(t); res(); };
    const fn = (id, info) => { if (id === tabId && info.status === "complete") done(); };
    const t = setTimeout(done, ms);
    chrome.tabs.onUpdated.addListener(fn);
  });
}

async function refreshJob(job) {
  for (const r of refreshing.values()) if (r.job.id === job.id) return;
  const tab = await chrome.tabs.create({ url: job.page, active: false });
  const r = { job, cands: new Map() };
  refreshing.set(tab.id, r);
  chrome.tabs.update(tab.id, { muted: true }).catch(() => {});
  try {
    await waitLoad(tab.id, 30000);
    await sleep(5000); // oynatıcı medyayı istesin, yönlendirmeli indirmeler başlasın diye biraz bekle
    try {
      const res = await chrome.scripting.executeScript({
        target: { tabId: tab.id, allFrames: true },
        func: () => [...document.querySelectorAll("a[href], area[href], video, audio, source")].map((e) => e.href || e.currentSrc || e.src).filter((u) => u && /^https?:/.test(u)).slice(0, 3000),
      });
      for (const u of res.flatMap((x) => x?.result || [])) if (!r.cands.has(u)) r.cands.set(u, urlHdr.get(u) || {});
    } catch {}
    for (const it of (await tabMap(tab.id)).values()) r.cands.set(it.url, it.headers || {});
  } finally {
    refreshing.delete(tab.id);
    chrome.tabs.remove(tab.id).catch(() => {});
  }
  // yüzlerce link olabiliyor, sadece adı ya da host'u tutanlar ve ağda görülenler
  const oldTail = tailOf(job.url), name = job.name.toLowerCase(), host = hostOf(job.url);
  const list = [...r.cands].filter(([u, h]) => Object.keys(h).length || tailOf(u) === oldTail || tailOf(u) === name || hostOf(u) === host).slice(0, 40);
  const candidates = [];
  for (const [url, h] of list) {
    const headers = { ...h };
    if (!Object.keys(headers).some((k) => /^cookie$/i.test(k))) {
      const c = await cookiesFor(url);
      if (c) headers.Cookie = c;
    }
    if (!Object.keys(headers).some((k) => /^referer$/i.test(k))) headers.Referer = job.page;
    candidates.push({ url, headers });
  }
  await send("/refresh-result", { id: job.id, candidates });
}

// video paneli, kaynak sırası
// uygulamadaki MEDIA_HOSTS'la aynı. bu sitelerde sayfa linkini yt-dlp'ye vermek daha iyi
const MEDIA_HOSTS = ["youtube.com", "youtu.be", "soundcloud.com", "vimeo.com", "tiktok.com", "instagram.com", "twitter.com", "x.com", "twitch.tv", "dailymotion.com", "reddit.com", "facebook.com", "fb.watch", "bilibili.com", "bandcamp.com", "mixcloud.com", "kick.com", "pinterest.com", "vk.com", "rumble.com", "streamable.com", "threads.net", "bsky.app", "9gag.com", "imgur.com", "odysee.com", "nicovideo.jp"];
const fmtB = (n) => { if (!n) return ""; const u = ["B", "KB", "MB", "GB", "TB"]; let i = 0; while (n >= 1024 && i < 4) { n /= 1024; i++; } return n.toFixed(i > 1 ? 1 : 0).replace(".", ",") + " " + u[i]; };
const fmtD = (s) => { if (!s) return ""; s = Math.round(s); const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60); return h ? `${h} sa ${m} dk` : m ? `${m} dk ${s % 60} sn` : `${s} sn`; };
const safeTitle = (t) => (t || "").replace(/[\\/:*?"<>|\u0000-\u001f]+/g, " ").replace(/\s+/g, " ").trim().slice(0, 150);

async function sourcesFor(tab, src, drmHint) {
  const m = await tabMap(tab.id);
  const pending = visible(m).filter((i) => i.kind === "stream" && !i.info);
  if (pending.length) await Promise.race([Promise.all(pending.map((i) => probeStream(tab.id, i))), sleep(3000)]);
  const items = visible(m);
  const rows = [];
  let drm = !!drmHint;
  for (const it of items.filter((i) => i.kind === "stream")) {
    const i = it.info;
    const kind = i?.kind === "dash" || /\.mpd/i.test(it.url) ? "DASH" : "HLS";
    if (i?.drm) { drm = true; continue; }
    const heights = [...new Map((i?.variants || []).filter((v) => v.height).map((v) => [v.height, v])).values()];
    if (heights.length) {
      for (const v of heights) {
        const size = i.duration && v.bandwidth ? (v.bandwidth * i.duration) / 8 : 0;
        rows.push({ q: v.height + "p", label: `${v.height}p video`, sub: [kind, i.live ? "canlı" : fmtD(i.duration), size ? "~" + fmtB(size) : ""].filter(Boolean).join(" · "), url: it.url, mode: "video", height: v.height, rank: 3e9 + v.height });
      }
    } else {
      rows.push({ q: i?.height ? i.height + "p" : kind, label: "Akış (en iyi kalite)", sub: [kind, i ? (i.live ? "canlı" : fmtD(i.duration)) : "", i?.size ? "~" + fmtB(i.size) : ""].filter(Boolean).join(" · "), url: it.url, mode: "video", rank: 2e9 });
    }
  }
  const cleanSrc = /^https?:/.test(src) ? cleanUrl(src) : "";
  for (const it of items.filter((i) => i.kind === "video" || i.kind === "audio")) {
    const mine = it.url === cleanSrc;
    rows.push({ q: (it.ext || it.kind).slice(0, 4).toUpperCase(), label: it.kind === "audio" ? "Ses dosyası" : mine ? "Bu video" : "Video dosyası", sub: [fmtB(it.size), hostOf(it.url)].filter(Boolean).join(" · "), url: it.url, mode: "direct", ext: it.ext, name: it.name, rank: (mine ? 1.5e9 : 0) + (it.size || 0) / 1e3 });
  }
  if (cleanSrc && !items.some((i) => i.url === cleanSrc)) rows.push({ q: (extOf(cleanSrc) || "VID").toUpperCase(), label: "Bu video", sub: hostOf(cleanSrc), url: cleanSrc, mode: "direct", ext: extOf(cleanSrc), rank: 1.5e9 });
  rows.sort((a, b) => b.rank - a.rank);
  const host = hostOf(tab.url);
  const supported = MEDIA_HOSTS.some((h) => host === h || host.endsWith("." + h));
  const page = { page: true, q: "★", label: supported ? "Sayfadan indir (en iyi kalite)" : "Sayfayı Trawl'da aç", sub: supported ? "Kalite, altyazı ve ses seçilebilir" : "Trawl sayfadaki her şeyi çözümler", url: tab.url, mode: "ask" };
  if (supported) rows.unshift(page);
  else rows.push(page);
  return { drm, rows };
}

// popup mesajları
async function itemFor(tabId, url) {
  return (await tabMap(tabId)).get(url) || null;
}

chrome.runtime.onMessage.addListener((msg, sender, reply) => {
  (async () => {
    if (msg.type === "state") {
      const ok = await ping(true);
      const m = await tabMap(msg.tabId);
      if (ok) {
        // kapalıyken yakalanan akışları şimdi çöz, 4 sn'den fazla bekleme
        const todo = visible(m).filter((i) => i.kind === "stream" && !i.info);
        if (todo.length) await Promise.race([Promise.all(todo.map((i) => probeStream(msg.tabId, i))), new Promise((r) => setTimeout(r, 4000))]);
      }
      badge(msg.tabId);
      const tab = await chrome.tabs.get(msg.tabId).catch(() => null);
      const host = hostOf(tab?.url);
      reply({ ok, items: visible(m), host, skip: onSite(host, alive.skip) });
    } else if (msg.type === "send") {
      const it = msg.tabId != null ? await itemFor(msg.tabId, msg.url) : null;
      reply({ ok: await sendUrl(msg.url, { referer: msg.referer, mode: msg.mode || "ask", title: msg.title || "", headers: it?.headers || null, filename: msg.mode === "direct" ? it?.name || "" : "", size: it?.size || 0 }) });
    } else if (msg.type === "grab") {
      const tab = await chrome.tabs.get(msg.tabId);
      reply({ ok: await grabTab(tab) });
    } else if (msg.type === "sources") {
      const ok = await ping(true);
      reply(ok ? { ok, ...(await sourcesFor(sender.tab, msg.src || "", msg.drm)) } : { ok: false, rows: [] });
    } else if (msg.type === "panelSend") {
      const row = msg.row || {};
      const it = await itemFor(sender.tab.id, row.url);
      const t = safeTitle(msg.title);
      const filename = row.mode === "direct" && t && row.ext ? `${t}.${row.ext}` : row.mode === "direct" ? it?.name || "" : "";
      reply({ ok: await sendUrl(row.url, { referer: sender.tab.url, mode: row.mode, title: t, headers: it?.headers || urlHdr.get(row.url) || null, filename, size: it?.size || 0, height: row.height || 0 }) });
    } else if (msg.type === "alt") {
      // Alt+tık kurala bakmaz. ne olduğunu uygulama kendisi anlıyor
      reply({ ok: await sendUrl(msg.url, { referer: sender.url || sender.tab?.url || "", headers: urlHdr.get(msg.url) || null, title: msg.text || "" }) });
    } else if (msg.type === "site") {
      const r = await send("/site", { host: msg.host, skip: msg.skip });
      if (r?.skip) alive.skip = r.skip;
      reply({ ok: !!r });
    } else if (msg.type === "yt") {
      reply({ ok: await sendUrl(msg.url, { referer: sender.tab?.url || "", title: msg.title || "", mode: msg.mode || "ask" }) });
    }
  })();
  return true;
});
