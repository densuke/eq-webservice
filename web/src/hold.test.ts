import { test } from "node:test";
import assert from "node:assert/strict";
import { holdMs, mainMapTarget, subMapSigs, subMapState } from "./hold.ts";
import { SUB_MAP } from "./layout.ts";

test("hold time follows the max scale (boundaries fall on the right side)", () => {
  const want: [number, number][] = [[10, 60], [20, 60], [30, 180], [40, 180], [45, 600], [50, 600], [55, 900], [60, 900], [70, 900], [-1, 180], [0, 180]];
  for (const [scale, sec] of want) assert.equal(holdMs(scale, SUB_MAP), sec * 1000, `scale ${scale}`);
});

test("subMapState: 保持時間の内は 1、過ぎたら 0.4 から 0 へ直線で薄くなり 0.05 刻みに切り下げ、終わったら null", () => {
  const g = { key: "a", kind: "quake", updatedAt: 1000 };
  // 震度 2: 保持 60 秒、fadedAlpha 0.4、fadeSec 120 (SUB_MAP)。alpha = 0.4 * (1 - (e - 60000) / 120000)
  const at = (e: number, kind = "quake") => subMapState(1000 + e, { ...g, kind }, 20, SUB_MAP);
  assert.deepEqual(at(0), { key: "a", alpha: 1 });
  assert.deepEqual(at(60000), { key: "a", alpha: 1 });
  assert.deepEqual(at(60001), { key: "a", alpha: 0.35 }); // 0.3999967 は 0.05 刻みに切り下げて 0.35
  assert.deepEqual(at(60000 + 6000), { key: "a", alpha: 0.35 }); // 0.38
  assert.deepEqual(at(60000 + 60000), { key: "a", alpha: 0.2 }); // ちょうど 0.2 (浮動小数の誤差で 0.15 にならない)
  assert.deepEqual(at(60000 + 90000), { key: "a", alpha: 0.1 }); // 0.1 ちょうど
  assert.deepEqual(at(60000 + 119999), null); // 0.0000033 は切り下げて 0
  assert.deepEqual(at(60000 + 100000), { key: "a", alpha: 0.05 }); // 0.0667
  assert.equal(at(60000 + 111000), null); // 0.03 は切り下げて 0 = 消える
});

test("subMapState: 他の種別・地震なしは null、EEW も同じ", () => {
  const g = { key: "a", kind: "quake", updatedAt: 1000 };
  assert.deepEqual(subMapState(1000, { ...g, kind: "eew" }, 70, SUB_MAP), { key: "a", alpha: 1 });
  assert.equal(subMapState(1000, undefined, 20, SUB_MAP), null);
  assert.equal(subMapState(1000, { ...g, kind: "tsunami" }, 20, SUB_MAP), null);
  assert.equal(subMapState(1000 + 180000, g, 20, SUB_MAP), null);
});

test("subMapState: fadeSec が違えば薄くなる速さも変わる", () => {
  const g = { key: "a", kind: "quake", updatedAt: 0 };
  const cfg = { ...SUB_MAP, fadeSec: 40 };
  assert.deepEqual(subMapState(60000 + 20000, g, 20, cfg), { key: "a", alpha: 0.2 });
  assert.equal(subMapState(60000 + 40000, g, 20, cfg), null);
});

test("subMapSigs: 薄くなるのは fade の署名だけ変え、paint の署名は変えない", () => {
  const g = { key: "a", kind: "quake", updatedAt: 1000 };
  const at = (e: number, w = 300, h = 200) => subMapSigs(subMapState(1000 + e, g, 20, SUB_MAP), g, w, h);
  const solid = at(1000);
  const faded = at(60000 + 60000);
  assert.equal(solid.paint, faded.paint);
  assert.equal(solid.fade, "1");
  assert.equal(faded.fade, "0.2");
  assert.notEqual(solid.paint, at(1000, 301, 200).paint);
  const g2 = { ...g, updatedAt: 1500 };
  assert.notEqual(solid.paint, subMapSigs(subMapState(2000, g2, 20, SUB_MAP), g2, 300, 200).paint);
  assert.deepEqual(subMapSigs(null, undefined, 300, 200), { paint: "|300x200", fade: "" });
});

test("mainMapTarget: 左の地図はサブの地図が見えている間、履歴で選んだとき以外は日本全体のまま", () => {
  const box = { x: 1, y: 2, w: 3, h: 4 };
  assert.equal(mainMapTarget(box, true, false), null);
  assert.equal(mainMapTarget(box, true, true), box);
  assert.equal(mainMapTarget(box, false, false), box);
  assert.equal(mainMapTarget(box, false, true), box);
  assert.equal(mainMapTarget(null, true, true), null);
  assert.equal(mainMapTarget(null, false, false), null);
});
