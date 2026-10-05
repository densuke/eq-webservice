import { test } from "node:test";
import assert from "node:assert/strict";
import { type LayoutNode, type Stack, LAYOUTS, flexOf, pickLayout } from "./layout.ts";
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
  const fromStack = (s: Stack): string[] => s.items.flatMap((i) => (typeof i === "string" ? [i] : "slot" in i ? [i.slot] : fromStack(i)));
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
