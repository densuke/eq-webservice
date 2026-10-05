// web の画面の部品の座標を実測して、docs/ui-spec/measured.js とスクリーンショットを作り直す。
// 使い方 (リポジトリの直下から。Node 22 + Playwright が必要。リポジトリの依存には含めていない):
//   cd web && npm ci && npm run build && (cd dist && python3 -m http.server 8099 &)
//   node docs/ui-spec/tools/measure.mjs [--base http://127.0.0.1:8099/] [--out docs/ui-spec]
// API は固定の値で差し替える (WebSocket は繋がらないので、時計は「切断中」の見た目になる)。
import { createRequire } from "node:module";
import fs from "node:fs";
import path from "node:path";

const arg = (name, dflt) => { const i = process.argv.indexOf(`--${name}`); return i > 0 ? process.argv[i + 1] : dflt; };
const BASE = arg("base", "http://127.0.0.1:8099/");
const OUT = arg("out", "docs/ui-spec");
const DIST = arg("dist", "web/dist");
const require = createRequire(import.meta.url);
let chromium;
try { ({ chromium } = require("playwright")); } catch { ({ chromium } = createRequire("/opt/node22/lib/node_modules/")("playwright")); }

// ---- 差し替える API の値 ----
const geo = JSON.parse(fs.readFileSync(path.join(DIST, "warning-areas.geojson"), "utf8"));
const code = (n) => geo.features.find((f) => f.properties.name.includes(n))?.properties.code;
const warnings = { reported_at: "2026-10-05T17:00:00+09:00", areas: {}, names: {} };
for (const [n, kind] of [["石狩", "大雨警報"], ["札幌", "大雨注意報"], ["那覇", "強風注意報"]]) if (code(n)) warnings.areas[code(n)] = [{ code: "00", name: kind }];
const city = (name, lat, lon, temp, tmax) => ({ name, lat, lon, code: "100", text: "晴れ", temp, precip1h: null, tomorrow: { code: "200", text: "くもり", temp_min: tmax - 6, temp_max: tmax, pop: 30 } });
const weather = { observed_at: "2026-10-05T17:00:00+09:00", rain: [[35.7, 139.7, 3]], cities: [
  city("札幌", 43.06, 141.35, 14, 18), city("仙台", 38.27, 140.87, 17, 21), city("東京", 35.69, 139.69, 22, 25), city("千葉", 35.6, 140.1, 22, 25),
  city("名古屋", 35.18, 136.91, 23, 26), city("大阪", 34.69, 135.5, 24, 27), city("神戸", 34.69, 135.19, 24, 27), city("広島", 34.4, 132.46, 23, 26),
  city("福岡", 33.59, 130.4, 24, 27), city("那覇", 26.21, 127.68, 28, 30)] };
const banners = { interval_sec: 20, items: [{ image: null, text: "お知らせ: 毎日 4:00〜4:15 ごろ配信を再起動します", link: null }] };

// ---- 測る部品 (data.js の sel と同じもの) ----
const SEL = ["header.topbar", ".topbar h1", "#mode", "#back-live", "#overview", "#calm-now", "#wave-info", "#telop", "#settings-open", "#demo-open", "#sound", "#bgm", "#bgm-now",
  "#eew-banner", "#tsunami-banner", "#warn-banner", "main.layout", "#map", "svg.map", "#countdown", "#tour-toast", "#weather-caption", "#sound-hint", "#clock", ".legend", ".legend .scale",
  "#legend-tsunami", "#legend-wave", "#legend-warn", ".inset-okinawa", ".inset-ogasawara", ".offscreen-layer", "aside.side", "#settings-panel", "#demo-panel", "#detail", ".list-head", "#list", "#banner", "footer.credit"];
// [キー, viewport, クエリ, 待つ時間 (ms), 画像名]
const RUNS = [
  ["pc1440/calm", [1440, 900], "", 2500, "pc-calm"],
  ["hd1280/calm", [1280, 720], "", 2500, null],
  ["phone/calm", [390, 844], "", 2500, "phone-calm"],
  ["phoneLand/calm", [844, 390], "", 2500, "phone-landscape-calm"],
  ["tablet/calm", [800, 1000], "", 2500, null],
  ["pc1440/eew", [1440, 900], "?demo=standard", 24000, "pc-eew"],
  ["phone/eew", [390, 844], "?demo=tsunami", 24000, "phone-eew"],
];

const browser = await chromium.launch();
const out = {};
for (const [key, [w, h], query, wait, shot] of RUNS) {
  const ctx = await browser.newContext({ viewport: { width: w, height: h }, hasTouch: key.startsWith("phone") });
  const page = await ctx.newPage();
  await page.route("**/api/warnings", (r) => r.fulfill({ json: warnings }));
  await page.route("**/api/weather", (r) => r.fulfill({ json: weather }));
  await page.route("**/api/banners", (r) => r.fulfill({ json: banners }));
  await page.route("**/api/telop", (r) => r.fulfill({ json: ["出典: P2P地震情報 (気象庁発表)", "地図: 地球地図日本 (国土地理院) を加工"] }));
  await page.goto(BASE + query);
  await page.waitForTimeout(wait);
  const rects = await page.evaluate((sels) => Object.fromEntries(sels.flatMap((s) => {
    const el = document.querySelector(s);
    if (!el) return [];
    const r = el.getBoundingClientRect(), cs = getComputedStyle(el);
    if (el.hidden || cs.display === "none" || r.width === 0) return [];
    return [[s, { x: Math.round(r.x), y: Math.round(r.y + scrollY), w: Math.round(r.width), h: Math.round(r.height), pos: cs.position }]];
  })), SEL);
  out[key] = { doc: await page.evaluate(() => [document.documentElement.scrollWidth, document.documentElement.scrollHeight]), rects };
  if (shot) {
    fs.mkdirSync(path.join(OUT, "img"), { recursive: true });
    await page.screenshot({ path: path.join(OUT, "img", `${shot}.png`), fullPage: key.startsWith("phone/") });
  }
  await ctx.close();
}
await browser.close();
fs.writeFileSync(path.join(OUT, "measured.js"), `// 自動生成: tools/measure.mjs の出力。手で直さない。\nwindow.MEASURED = ${JSON.stringify(out, null, 1)};\n`);
console.log("wrote", Object.keys(out).join(", "));
