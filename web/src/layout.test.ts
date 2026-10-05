import { test } from "node:test";
import assert from "node:assert/strict";
import { type LayoutNode, type Stack, LAYOUTS, flexOf, pickLayout } from "./layout.ts";
import { BOXES, SLOTS } from "./layout-dom.ts";

test("the layout is picked by the screen size (same 800px boundary as the CSS)", () => {
  assert.equal(pickLayout(LAYOUTS, 1440, 900).name, "regular");
  assert.equal(pickLayout(LAYOUTS, 801, 600).name, "regular");
  assert.equal(pickLayout(LAYOUTS, 800, 1000).name, "compact");
  assert.equal(pickLayout(LAYOUTS, 390, 844).name, "compact");
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

/** 定義の中の部品の名前 (重ね物を含む) */
function slots(n: LayoutNode): string[] {
  const fromStack = (s: Stack): string[] => s.items.flatMap((i) => (typeof i === "string" ? [i] : fromStack(i)));
  return [
    ...(n.slot ? [n.slot] : []),
    ...Object.values(n.overlays ?? {}).flatMap((s) => (s ? fromStack(s) : [])),
    ...(n.children ?? []).flatMap(slots),
  ];
}

for (const l of LAYOUTS) {
  test(`${l.name}: every part exists and is placed exactly once`, () => {
    const used = slots(l.root);
    for (const s of used) assert.ok(SLOTS[s], `unknown part ${s}`);
    assert.deepEqual(used.filter((s, i) => used.indexOf(s) !== i), [], "placed twice");
    // ページの部品は全部どこかに置く (置き忘れると要素が元の場所に残り、並びが崩れる)
    assert.deepEqual(Object.keys(SLOTS).filter((s) => !used.includes(s)), []);
  });
  test(`${l.name}: boxes exist`, () => {
    const boxes = (n: LayoutNode): string[] => [...(n.box ? [n.box] : []), ...(n.children ?? []).flatMap(boxes)];
    for (const b of boxes(l.root)) assert.ok(BOXES[b], `unknown box ${b}`);
  });
}

test("every SLOTS/BOXES selector names an element of index.html (or an inset made by map.ts)", async () => {
  const { readFileSync } = await import("node:fs");
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
