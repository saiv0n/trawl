// chromium'lar için extension-dist/chrome, bir de extension-dist/firefox
// npm run ext:build, mağaza zip'lerini de çıkarıyor
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const src = join(root, "extension");
const out = join(root, "extension-dist");
const manifest = JSON.parse(readFileSync(join(src, "manifest.json"), "utf8"));

const targets = {
  chrome: (m) => m,
  firefox: (m) => {
    const f = structuredClone(m);
    delete f.key; // chromium'a özel, firefox'ta kimlik gecko.id'den
    f.background = { scripts: ["background.js"] }; // firefox MV3'te service worker yok, olay sayfası
    f.browser_specific_settings = { gecko: { id: "trawl@trawl.app", strict_min_version: "128.0" } }; // MAIN world içerik betiği 128'de geldi
    return f;
  },
};

rmSync(out, { recursive: true, force: true });
for (const [name, fix] of Object.entries(targets)) {
  const dir = join(out, name);
  mkdirSync(dir, { recursive: true });
  cpSync(src, dir, { recursive: true, filter: (p) => !p.endsWith("manifest.json") });
  writeFileSync(join(dir, "manifest.json"), JSON.stringify(fix(manifest), null, 2) + "\n");
  const zip = join(out, `trawl-${name}-${manifest.version}.zip`);
  execFileSync("powershell", ["-NoProfile", "-Command", `Compress-Archive -Path '${dir}\\*' -DestinationPath '${zip}' -Force`], { stdio: "inherit" });
  console.log(`${name}: ${zip}`);
}
