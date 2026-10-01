import { test } from "node:test";
import assert from "node:assert/strict";
import { thinBoxes } from "./thin.ts";

const r = (x0: number, y0: number, x1: number, y1: number) => ({ x0, y0, x1, y1 });

test("boxes that do not overlap are all kept", () => {
  assert.deepEqual(thinBoxes([r(0, 0, 10, 10), r(10, 0, 20, 10), r(0, 20, 10, 30)], []), [true, true, true]);
});

test("of two overlapping boxes the earlier (higher priority) stays and the later is dropped", () => {
  assert.deepEqual(thinBoxes([r(0, 0, 10, 10), r(5, 5, 15, 15)], []), [true, false]);
});

test("a dropped box does not block the ones after it", () => {
  assert.deepEqual(thinBoxes([r(0, 0, 10, 10), r(5, 5, 15, 15), r(12, 12, 20, 20)], []), [true, false, true]);
});

test("a box overlapping a blocker is dropped even if it comes first", () => {
  assert.deepEqual(thinBoxes([r(0, 0, 10, 10), r(30, 0, 40, 10)], [r(8, 8, 12, 12)]), [false, true]);
});

test("no boxes, no result", () => {
  assert.deepEqual(thinBoxes([], [r(0, 0, 1, 1)]), []);
});
