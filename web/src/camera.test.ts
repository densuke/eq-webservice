import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_STOP_KM, followRadiusKm, KM_TO_UNITS, MIN_RADIUS_KM, pad, pointBox, ringsBox, stopRadiusKm, union } from "./camera.ts";

test("follow radius starts at the minimum, grows with the S wave and stops", () => {
  assert.equal(followRadiusKm(null, 400), MIN_RADIUS_KM);
  assert.equal(followRadiusKm(10, 400), MIN_RADIUS_KM);
  assert.equal(followRadiusKm(250, 400), 250);
  assert.equal(followRadiusKm(900, 400), 400);
});

test("stop radius reaches the farthest corner of the shaken area", () => {
  assert.equal(stopRadiusKm(0, 0, null), DEFAULT_STOP_KM);
  const r = stopRadiusKm(0, 0, { x0: -300 * KM_TO_UNITS, y0: 0, x1: 100 * KM_TO_UNITS, y1: 400 * KM_TO_UNITS });
  assert.ok(Math.abs(r - 500) < 1e-9, `${r}`);
  // 震央のすぐ近くだけが揺れた場合でも最小半径は保つ
  assert.equal(stopRadiusKm(0, 0, pointBox(0, 0, 1)), MIN_RADIUS_KM);
});

test("union and pad", () => {
  assert.equal(union(null, null), null);
  const a = { x0: 0, y0: 0, x1: 1, y1: 1 };
  assert.deepEqual(union(a, null), a);
  assert.deepEqual(union(a, { x0: -1, y0: 0.5, x1: 0.5, y1: 3 }), { x0: -1, y0: 0, x1: 1, y1: 3 });
  const p = pad({ x0: 0, y0: 0, x1: 1000, y1: 10 });
  assert.ok(p.x1 - p.x0 > 1000);
  assert.ok(Math.abs(p.y1 - p.y0 - 2 * MIN_RADIUS_KM * KM_TO_UNITS) < 1e-9);
});

test("rings box covers every ring, so an area of many islands is framed whole", () => {
  assert.equal(ringsBox([]), null);
  const islandA: [number, number][] = [[0, 0], [1, 0], [1, 1]];
  const islandB: [number, number][] = [[10, -5], [12, -4]];
  assert.deepEqual(ringsBox([islandA, islandB]), { x0: 0, y0: -5, x1: 12, y1: 1 });
});
