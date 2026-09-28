import { test } from "node:test";
import assert from "node:assert/strict";
import { countdown, countdownWorthShowing, estimateIntensity, intensityToScale, nearestArea, notifyScale, shouldNotify } from "./personal.ts";
import type { Station } from "./detail.ts";

const stations = new Map<string, Station>([
  ["水戸市金町", { lat: 36.37, lon: 140.47, area: "茨城県北部" }],
  ["つくば市天王台", { lat: 36.11, lon: 140.1, area: "茨城県南部" }],
]);

test("the home area is the area of the nearest station", () => {
  assert.equal(nearestArea({ lat: 36.08, lon: 140.12 }, stations), "茨城県南部");
  assert.equal(nearestArea({ lat: 36.4, lon: 140.4 }, stations), "茨城県北部");
  assert.equal(nearestArea({ lat: 36.4, lon: 140.4 }, new Map()), null);
});

test("notification levels", () => {
  const eewWarn = { warning: true, maxScale: 50, homeScale: null };
  const big = { warning: false, maxScale: 40, homeScale: null };
  const small = { warning: false, maxScale: 30, homeScale: null };
  const smallButHome = { warning: false, maxScale: 30, homeScale: 30 };
  assert.equal(shouldNotify("off", eewWarn), false);
  assert.equal(shouldNotify("warning", eewWarn), true);
  assert.equal(shouldNotify("warning", big), false);
  assert.equal(shouldNotify("4", big), true);
  assert.equal(shouldNotify("4", small), false);
  assert.equal(shouldNotify("4", smallButHome), true);
  assert.equal(shouldNotify("3", small), true);
  assert.equal(shouldNotify("3", { warning: false, maxScale: 20, homeScale: null }), false);
});

test("tsunami grades map to notification scales", () => {
  assert.equal(notifyScale.tsunami("watch"), 30);
  assert.equal(notifyScale.tsunami("warning"), 45);
  assert.equal(notifyScale.tsunami("major_warning"), 70);
});

test("countdown to the S wave at home", () => {
  const origin = 1_000_000;
  // 震源の深さ 0、震央から 37.5 km → S波 (3.75 km/s) は 10 秒後
  const home = { lat: 36.0, lon: 140.0 };
  const epi = { lat: 36.0 + 37.5 / 111.2, lon: 140.0, depth: 0 };
  const c = countdown(home, epi, origin, origin + 4000);
  assert.ok(Math.abs(c.remainingSec - 6) < 0.1, `${c.remainingSec}`);
  assert.equal(c.arrived, false);
  assert.equal(countdown(home, epi, origin, origin + 12_000).arrived, true);
});

test("estimated intensity falls with distance and rises with magnitude", () => {
  // 宮城県沖 M6.8 深さ 40km → 仙台 (約 100km): 震度4 程度
  const near = estimateIntensity(6.8, 40, 99);
  assert.ok(near > 3.5 && near < 4.5, `${near}`);
  // 千葉 M5.0 深さ 50km → 神戸 (約 450km): ほとんど揺れない
  const far = estimateIntensity(5.0, 50, 450);
  assert.ok(far < 1, `${far}`);
  assert.ok(estimateIntensity(7.5, 40, 99) > near);
  assert.ok(estimateIntensity(6.8, 40, 300) < near);
});

test("countdown is shown when jma predicts shaking at home or the estimate reaches intensity 3", () => {
  assert.equal(countdownWorthShowing(30, null), true);
  assert.equal(countdownWorthShowing(10, null), true); // 気象庁が地域に含めている
  assert.equal(countdownWorthShowing(null, 3.9), true);
  assert.equal(countdownWorthShowing(null, 2.5), true);
  assert.equal(countdownWorthShowing(null, 2.4), false);
  assert.equal(countdownWorthShowing(null, null), false);
});

test("instrumental intensity to scale class", () => {
  assert.equal(intensityToScale(0.4), 0);
  assert.equal(intensityToScale(2.5), 30);
  assert.equal(intensityToScale(4.6), 45);
  assert.equal(intensityToScale(5.2), 50);
  assert.equal(intensityToScale(6.6), 70);
});
