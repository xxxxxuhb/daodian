import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const DIR = dirname(fileURLToPath(import.meta.url));
const OUT = join(DIR, "out");
if (!existsSync(OUT)) mkdirSync(OUT, { recursive: true });

const CHROME = [
  "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
  "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
].find(existsSync);

const SIZES = { "spec.html": [1400, 680], "overview.html": [1400, 660] };
const targets = process.argv.slice(2);
if (targets.length === 0) { console.error("usage: node render.mjs <file.html> ..."); process.exit(1); }

for (const t of targets) {
  const html = join(DIR, t);
  const png = join(OUT, t.replace(/\.html$/, ".png"));
  mkdirSync(dirname(png), { recursive: true });
  const url = "file:///" + html.replace(/\\/g, "/");
  const started = Date.now();
  const [vw, vh] = SIZES[t.split(/[\\/]/).pop()] || [1400, 880];
  execFileSync(CHROME, [
    "--headless=new", "--disable-gpu", "--hide-scrollbars", "--no-sandbox",
    "--force-device-scale-factor=2", "--window-size=" + vw + "," + vh,
    "--virtual-time-budget=2500",
    "--screenshot=" + png, url,
  ], { stdio: "ignore" });
  const st = statSync(png);
  console.log("shot", t, "->", st.size, "bytes", Math.round((Date.now() - started) / 1000) + "s");
}
