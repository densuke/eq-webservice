import { test } from "node:test";
import assert from "node:assert/strict";
import { holdMs, mainMapTarget, subMapSigs, subMapState } from "./hold.ts";
import { SUB_MAP } from "./layout.ts";

test("hold time follows the max scale (boundaries fall on the right side)", () => {
  const want: [number, number][] = [[10, 60], [20, 60], [30, 180], [40, 180], [45, 600], [50, 600], [55, 900], [60, 900], [70, 900], [-1, 180], [0, 180]];
  for (const [scale, sec] of want) assert.equal(holdMs(scale, SUB_MAP), sec * 1000, `scale ${scale}`);
});

test("subMapState is solid inside the hold time, faded after, null when not a quake", () => {
  const g = { key: "a", kind: "quake", updatedAt: 1000 };
  assert.deepEqual(subMapState(1000 + 60000, g, 20, SUB_MAP), { key: "a", faded: false });
  assert.deepEqual(subMapState(1000 + 60001, g, 20, SUB_MAP), { key: "a", faded: true });
  assert.deepEqual(subMapState(1000, { ...g, kind: "eew" }, 70, SUB_MAP), { key: "a", faded: false });
  assert.equal(subMapState(1000, undefined, 20, SUB_MAP), null);
  assert.equal(subMapState(1000, { ...g, kind: "tsunami" }, 20, SUB_MAP), null);
});

test("subMapSigs: fading changes only the fade signature, not the paint one", () => {
  const g = { key: "a", kind: "quake", updatedAt: 1000 };
  const at = (now: number, w = 300, h = 200, alpha = 0.4) => subMapSigs(subMapState(now, g, 20, SUB_MAP), g, w, h, alpha);
  const solid = at(2000);
  const faded = at(1000 + 60001);
  assert.equal(solid.paint, faded.paint);
  assert.notEqual(solid.fade, faded.fade);
  assert.notEqual(solid.fade, at(2000, 300, 200, 0.5).fade);
  assert.notEqual(solid.paint, at(2000, 301, 200).paint);
  const g2 = { ...g, updatedAt: 1500 };
  assert.notEqual(solid.paint, subMapSigs(subMapState(2000, g2, 20, SUB_MAP), g2, 300, 200, 0.4).paint);
  assert.equal(subMapSigs(null, undefined, 300, 200, 0.4).paint, "|300x200");
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
