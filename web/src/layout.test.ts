import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { type Layout, type LayoutNode, LAYOUTS, REQUIRED_SLOTS, checkLayouts, chooseLayout, flexOf, pickLayout, slotsOf } from "./layout.ts";
import { BOXES, SLOTS } from "./layout-dom.ts";

test("the layout is picked by the screen size (same 800px boundary as the CSS)", () => {
  assert.equal(pickLayout(LAYOUTS, 1440, 900).name, "regular");
  assert.equal(pickLayout(LAYOUTS, 801, 600).name, "regular");
  assert.equal(pickLayout(LAYOUTS, 800, 1000).name, "compact");
  assert.equal(pickLayout(LAYOUTS, 390, 844).name, "compact");
  // 横向きのスマホ (幅 801 以上で高さ 480 以下) は landscape
  assert.equal(pickLayout(LAYOUTS, 915, 350).name, "landscape");
  assert.equal(pickLayout(LAYOUTS, 844, 390).name, "landscape");
  assert.equal(pickLayout(LAYOUTS, 801, 480).name, "landscape");
  assert.equal(pickLayout(LAYOUTS, 801, 481).name, "regular");
});

test("a manual layout is chosen only by name (?layout=)", () => {
  const ls = [
    { name: "trial", manual: true, root: { slot: "main" } },
    { name: "wide", when: { minWidth: 801 }, root: { slot: "main" } },
    { name: "narrow", root: { slot: "main" } },
  ] as Layout[];
  assert.equal(pickLayout(ls, 1440, 900).name, "wide");
  assert.equal(pickLayout(ls, 390, 844).name, "narrow");
  // どれにも合わないときも manual は選ばない
  assert.equal(pickLayout([ls[0], ls[1]], 390, 844).name, "wide");
  assert.equal(chooseLayout(ls, 390, 844, "trial").name, "trial");
  assert.equal(chooseLayout(ls, 390, 844, "wide").name, "wide");
  for (const f of [null, "", "nope"]) assert.equal(chooseLayout(ls, 390, 844, f).name, "narrow", String(f));
});

test("sizes become flex values", () => {
  assert.equal(flexOf("fill", false), "1 1 0");
  assert.equal(flexOf("fill:2", false), "2 1 0");
  assert.equal(flexOf(undefined, false), "1 1 0");
  assert.equal(flexOf("auto", false), "0 1 auto");
  assert.equal(flexOf("380px", false), "0 0 380px");
  assert.equal(flexOf("60svh", true), "0 0 60svh");
  // ページ全体がスクロールする縦の並びでは、残りを分けず中身の大きさ
  assert.equal(flexOf("fill", true), "0 1 auto");
});

test("slotsOf lists parts in order, including overlays and nested stacks", () => {
  const node: LayoutNode = { slot: "main", overlays: { "top-left": { flow: "column", items: ["inset", { slot: "banners", variant: "compact" }, { flow: "row", items: ["legend"] }] } } };
  assert.deepEqual(slotsOf(node), ["main", "inset", "banners", "legend"]);
});

/** 出荷している自動の定義が置く部品 (省いたら見た目が変わる。新しい部品はここに足さない限り省いてよい) */
const SHIPPED_PARTS = ["topbar", "banners", "main", "settings", "detail", "history-head", "history", "notice", "credit",
  "inset", "ogasawara", "caption", "countdown", "legend", "clock", "toast", "hint"];

for (const l of LAYOUTS) {
  test(`${l.name}: every part exists, is placed at most once, and main is placed`, () => {
    const used = slotsOf(l.root);
    for (const s of used) assert.ok(SLOTS[s], `unknown part ${s}`);
    assert.deepEqual(used.filter((s, i) => used.indexOf(s) !== i), [], "placed twice");
    assert.deepEqual(REQUIRED_SLOTS.filter((s) => !used.includes(s)), []);
  });
  test(`${l.name}: boxes exist`, () => {
    const boxes = (n: LayoutNode): string[] => [...(n.box ? [n.box] : []), ...(n.children ?? []).flatMap(boxes)];
    for (const b of boxes(l.root)) assert.ok(BOXES[b], `unknown box ${b}`);
  });
}

for (const name of ["landscape", "regular", "compact"]) {
  test(`${name}: places every shipped part`, () => {
    const l = LAYOUTS.find((x) => x.name === name)!;
    assert.deepEqual(SHIPPED_PARTS.filter((s) => !slotsOf(l.root).includes(s)), []);
  });
}

test("every SLOTS/BOXES selector names an element of index.html (or an inset made by map.ts)", () => {
  const html = readFileSync(new URL("../public/index.html", import.meta.url), "utf8");
  const mapTs = readFileSync(new URL("./map.ts", import.meta.url), "utf8");
  for (const sel of [...Object.values(SLOTS), ...Object.values(BOXES)].flatMap((s) => s.split(","))) {
    const m = /^\s*([a-z]*)([#.])([\w-]+)\s*$/.exec(sel);
    assert.ok(m, `simple selector expected: ${sel}`);
    const [, tag, kind, name] = m;
    const inset = /^inset-(\w+)$/.exec(name);
    if (inset) {
      assert.match(mapTs, new RegExp(`id: "${inset[1]}"`), sel);
      continue;
    }
    const attr = kind === "#" ? `id="${name}"` : `class="(?:[^"]* )?${name}(?: [^"]*)?"`;
    assert.match(html, new RegExp(`<${tag || "[a-z]+"}\\b[^>]*\\b${attr}`), sel);
  }
});

const check = (data: unknown) => checkLayouts(data, Object.keys(SLOTS), Object.keys(BOXES));
const shipped = () => JSON.parse(readFileSync(new URL("./layout.json", import.meta.url), "utf8"));

test("the shipped layout file (layout.json) passes the check and is the built-in", () => {
  assert.deepEqual(check(shipped()), []);
  assert.deepEqual(shipped().layouts, LAYOUTS);
});

test("the check names what is wrong in a layout file", () => {
  const broken = (edit: (d: any) => void) => {
    const d = shipped();
    edit(d);
    return check(d).join("\n");
  };
  // 知らない部品・置いていない部品・2 回置いた部品
  assert.match(broken((d) => (d.layouts[1].root.children[0].slot = "topbarr")), /知らない部品 "topbarr"/);
  // 必須でない部品は省いてよい。main は省けない
  assert.equal(broken((d) => d.layouts[2].root.children.splice(1, 1)), "");
  assert.match(broken((d) => (d.layouts[1].root.children[2].children[0] = { slot: "notice" })), /regular\): 部品「main」を置いていない/);
  assert.match(broken((d) => d.layouts[1].root.children.push({ slot: "clock" })), /「clock」を 2 回置いている/);
  // 積みの余白と場所取り (隅の積みと入れ子の積みの両方で使える)
  const corner = (d: any) => d.layouts[2].root.children[2].children[0].overlays["top-left"];
  assert.equal(broken((d) => Object.assign(corner(d), { gap: "8px", pad: "4px 8px" })), "");
  assert.equal(broken((d) => Object.assign(corner(d).items[0], { gap: "4px", minHeight: "140px" })), "");
  for (const bad of ["big", "8", "1px; color: red", "", "1px 2px 3px 4px 5px"]) {
    assert.match(broken((d) => (corner(d).pad = bad)), /pad .* は使えない/, bad);
  }
  assert.match(broken((d) => (corner(d).items[0].minHeight = "fill")), /minHeight .* は使えない/);
  assert.match(broken((d) => (corner(d).margin = "4px")), /知らないキー「margin」/);
  // 手動専用の印
  assert.equal(broken((d) => (d.layouts[1].manual = true)), "");
  assert.match(broken((d) => (d.layouts[1].manual = "yes")), /manual は true だけ/);
  assert.match(broken((d) => d.layouts.forEach((l: any) => (l.manual = true))), /manual でない定義が 1 つも無い/);
  // 大きさ・向き・容器・隅・条件・キーの書き間違い
  for (const size of ["380", "big", "fill:x", "1px; color: red", ""]) {
    assert.match(broken((d) => (d.layouts[1].root.children[2].children[1].size = size)), /大きさ .* は使えない/, size);
  }
  for (const size of ["380px", "fill:2", "auto", "60svh", "30%", "clamp(260px, 25vw, 380px)"]) {
    assert.equal(broken((d) => (d.layouts[1].root.children[2].children[1].size = size)), "", size);
  }
  assert.match(broken((d) => (d.layouts[1].root.children[2].dir = "grid")), /dir は "row" か "column"/);
  assert.match(broken((d) => (d.layouts[1].root.children[2].box = "aside")), /知らない容器 "aside"/);
  assert.match(broken((d) => (d.layouts[1].root.children[2].children[0].overlays.middle = { flow: "row", items: [] })), /知らない隅「middle」/);
  assert.match(broken((d) => (d.layouts[1].when = { minWidht: 801 })), /when\.minWidht は使えない/);
  assert.match(broken((d) => (d.layouts[1].root.sise = "auto")), /知らないキー「sise」/);
  assert.match(broken((d) => (d.version = 2)), /version は 1/);
  // ファイルの形そのもの
  assert.deepEqual(check(null), ["定義ファイルが JSON のオブジェクトでない"]);
  assert.match(check({ version: 1, layouts: [] }).join(), /layouts が空/);
});
