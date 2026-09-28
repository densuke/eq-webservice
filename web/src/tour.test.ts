import { test } from "node:test";
import assert from "node:assert/strict";
import { tourIndex, worthTouring } from "./tour.ts";

const miyagi = { lat: 38.3, lon: 142.0 };
const ishikawa = { lat: 37.5, lon: 137.2 };
const noto2 = { lat: 37.55, lon: 137.3 };

test("tour only when there are two or more earthquakes far apart", () => {
  assert.equal(worthTouring([miyagi]), false);
  // 能登の群発のように近い地震だけなら 1 画面に入るので巡回しない
  assert.equal(worthTouring([ishikawa, noto2]), false);
  assert.equal(worthTouring([miyagi, ishikawa]), true);
  assert.equal(worthTouring([ishikawa, noto2, miyagi]), true);
});

test("the index advances every interval and wraps around", () => {
  const start = 1_000_000;
  assert.equal(tourIndex(start, start, 10, 3), 0);
  assert.equal(tourIndex(start, start + 9_999, 10, 3), 0);
  assert.equal(tourIndex(start, start + 10_000, 10, 3), 1);
  assert.equal(tourIndex(start, start + 35_000, 10, 3), 0);
  assert.equal(tourIndex(start, start + 5_000, 5, 2), 1);
});
