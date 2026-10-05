// レイアウトの定義 (layout-def.js の regular) を描いた座標が、いまの web の実測 (measured.js の hd1280/calm) と合うかを確かめる。
// 使い方 (リポジトリの直下から): node docs/ui-spec/tools/check-layout.mjs   (Node 22 + Playwright)
import { createRequire } from "node:module";
import path from "node:path";
import fs from "node:fs";
const require = createRequire(import.meta.url);
let chromium;
try { ({ chromium } = require("playwright")); } catch { ({ chromium } = createRequire("/opt/node22/lib/node_modules/")("playwright")); }

const dir = path.resolve("docs/ui-spec");
const measured = (() => { const w = {}; new Function("window", fs.readFileSync(path.join(dir, "measured.js"), "utf8"))(w); return w.MEASURED; })();
const m = measured["hd1280/calm"].rects;
// 定義の部品名 → 実測のセレクタ
const PAIRS = { topbar: "header.topbar", banners: "#warn-banner", main: "#map", inset: ".inset-okinawa", caption: "#weather-caption", legend: ".legend", clock: "#clock", detail: "#detail", credit: "footer.credit" };
const TOL = 2;

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
await page.goto("file://" + path.join(dir, "layout-preview.html") + "?layout=regular&w=1280&h=720");
await page.waitForFunction(() => window.__layoutRects?.main);
const r = await page.evaluate(() => window.__layoutRects);
await browser.close();

let bad = 0;
for (const [slot, sel] of Object.entries(PAIRS)) {
  const a = r[slot], b = m[sel];
  // caption の幅は文言で変わるので位置だけ比べる
  const keys = slot === "caption" ? ["x", "y"] : ["x", "y", "w", "h"];
  const diff = keys.filter((k) => Math.abs(a[k] - b[k]) > TOL);
  console.log(`${diff.length ? "NG" : "ok"}  ${slot.padEnd(8)} 定義 ${a.x},${a.y} ${a.w}×${a.h}  実測 ${b.x},${b.y} ${b.w}×${b.h}${diff.length ? "  ずれ: " + diff.join(",") : ""}`);
  bad += diff.length ? 1 : 0;
}
// 右パネルの幅 (定義では容器なので history で見る)
const side = r.history.w === 380;
console.log(`${side ? "ok" : "NG"}  右パネル幅 ${r.history.w} (実測 380)`);
process.exit(bad || !side ? 1 : 0);
