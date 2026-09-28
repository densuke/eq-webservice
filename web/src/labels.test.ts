import { test } from "node:test";
import assert from "node:assert/strict";
import { interiorPoint, labelPx, mainRing, pickLabels } from "./labels.ts";

const inside = (p: [number, number], ring: [number, number][]) => {
  let c = false;
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    const [xi, yi] = ring[i];
    const [xj, yj] = ring[j];
    if (yi > p[1] !== yj > p[1] && p[0] < ((xj - xi) * (p[1] - yi)) / (yj - yi) + xi) c = !c;
  }
  return c;
};

test("interior point stays inside a concave shape (not the bounding box center)", () => {
  // コの字: 外接矩形の中心 (5,5) は切り欠きの中 (外側)
  const u: [number, number][] = [
    [0, 0],
    [10, 0],
    [10, 3],
    [3, 3],
    [3, 7],
    [10, 7],
    [10, 10],
    [0, 10],
    [0, 0],
  ];
  const p = interiorPoint([u]);
  assert.ok(inside(p, u), `${p}`);
});

test("interior point uses the largest ring (main island rather than a small islet)", () => {
  const big: [number, number][] = [
    [0, 0],
    [10, 0],
    [10, 10],
    [0, 10],
    [0, 0],
  ];
  const small: [number, number][] = [
    [50, 50],
    [51, 50],
    [51, 51],
    [50, 51],
    [50, 50],
  ];
  const p = interiorPoint([small, big]);
  assert.ok(p[0] < 10 && p[1] < 10, `${p}`);
});

test("overlapping labels: the stronger shaking wins", () => {
  const picked = pickLabels([
    { key: "a", x: 0, y: 0, w: 10, h: 10, scale: 30 },
    { key: "b", x: 5, y: 5, w: 10, h: 10, scale: 50 },
    { key: "c", x: 50, y: 50, w: 10, h: 10, scale: 10 },
  ]);
  assert.deepEqual(picked.map((l) => l.key), ["b", "c"]);
});

test("labels grow with intensity", () => {
  assert.ok(labelPx(70) > labelPx(45));
  assert.ok(labelPx(45) > labelPx(10));
});

test("main ring is the largest one, so far islands do not widen the camera", () => {
  const island: [number, number][] = [[0, 100], [1, 100], [1, 101], [0, 101]];
  const main: [number, number][] = [[0, 0], [10, 0], [10, 10], [0, 10]];
  assert.deepEqual(mainRing([island, main]), main);
});
