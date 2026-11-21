<script lang="ts" module>
  // Phosphor ikonları (MIT). her ikon belgeye bir kez <symbol> olarak giriyor, sonra sadece <use>
  const regular = import.meta.glob(
    "/node_modules/@phosphor-icons/core/assets/regular/{download-simple,pause,play,folder-simple,trash,x,arrow-clockwise,gear-six,link,magnifying-glass,video,music-note,file-text,app-window,file-zip,image,file,list,check,lightning,clock,puzzle-piece,clipboard-text,plus,minus,square,caret-up,caret-down,caret-right,copy,stack,broadcast,code,globe,binoculars,shield-check,power,info,package,funnel,arrow-line-up,warning,closed-captioning,scissors,arrow-counter-clockwise,hard-drives,link-break,command,arrow-square-out,dots-three,check-circle,x-circle,wifi-slash,tray,sliders-horizontal,circle-notch,keyboard,sidebar-simple}.svg",
    { query: "?raw", import: "default", eager: true },
  ) as Record<string, string>;
  const fill = import.meta.glob(
    "/node_modules/@phosphor-icons/core/assets/fill/{play-fill,pause-fill,check-circle-fill,warning-fill,x-circle-fill,list-fill,download-simple-fill,clock-fill,video-fill,music-note-fill,file-text-fill,app-window-fill,file-zip-fill,image-fill,file-fill}.svg",
    { query: "?raw", import: "default", eager: true },
  ) as Record<string, string>;

  const MAP: Record<string, string> = {
    down: "download-simple", pause: "pause", play: "play", folder: "folder-simple", trash: "trash", x: "x", retry: "arrow-clockwise",
    gear: "gear-six", link: "link", search: "magnifying-glass", video: "video", music: "music-note", doc: "file-text", app: "app-window",
    archive: "file-zip", image: "image", file: "file", list: "list", check: "check", bolt: "lightning", clock: "clock", puzzle: "puzzle-piece",
    paste: "clipboard-text", plus: "plus", min: "minus", max: "square", up: "caret-up", dn: "caret-down", right: "caret-right", copy: "copy",
    layers: "stack", stream: "broadcast", embed: "code", globe: "globe", radar: "binoculars", shield: "shield-check", power: "power",
    info: "info", box: "package", filter: "funnel", top: "arrow-line-up", warn: "warning", cc: "closed-captioning", scissors: "scissors",
    undo: "arrow-counter-clockwise", disk: "hard-drives", broken: "link-break", command: "command", external: "arrow-square-out",
    more: "dots-three", ok: "check-circle", fail: "x-circle", offline: "wifi-slash", tray: "tray", sliders: "sliders-horizontal",
    spin: "circle-notch", keyboard: "keyboard", panel: "sidebar-simple",
  };
  const P = "/node_modules/@phosphor-icons/core/assets/";
  let sprite: SVGSVGElement | null = null;
  const added = new Set<string>();

  /** sembolü bir kez ekleyip id döner */
  function symbol(name: string, filled: boolean): string {
    const base = MAP[name] || name;
    let key = filled && fill[`${P}fill/${base}-fill.svg`] ? `${base}-fill` : base;
    if (!regular[`${P}regular/${key}.svg`] && !fill[`${P}fill/${key}.svg`]) key = "file";
    const id = `ph-${key}`;
    if (added.has(id) || typeof document === "undefined") return id;
    const raw = key.endsWith("-fill") ? fill[`${P}fill/${key}.svg`] : regular[`${P}regular/${key}.svg`];
    if (!sprite) {
      sprite = document.createElementNS("http://www.w3.org/2000/svg", "svg");
      sprite.setAttribute("aria-hidden", "true");
      sprite.style.cssText = "position:absolute;width:0;height:0;overflow:hidden";
      document.body.prepend(sprite);
    }
    const inner = raw.replace(/^<svg[^>]*>/, "").replace(/<\/svg>\s*$/, "");
    const sym = document.createElementNS("http://www.w3.org/2000/svg", "symbol");
    sym.id = id;
    sym.setAttribute("viewBox", "0 0 256 256");
    sym.innerHTML = inner;
    sprite.appendChild(sym);
    added.add(id);
    return id;
  }
</script>

<script lang="ts">
  let { n, size = 16, filled = false, cls = "" }: { n: string; size?: number; filled?: boolean; cls?: string; sw?: number } = $props();
  const id = $derived(symbol(n, filled));
</script>

<svg class="ic {cls}" width={size} height={size} fill="currentColor" aria-hidden="true"><use href="#{id}" /></svg>

<style>
  .ic { display: inline-block; flex: none; vertical-align: middle; }
</style>
