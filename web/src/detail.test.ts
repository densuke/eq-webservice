import { test } from "node:test";
import assert from "node:assert/strict";
import { eewAreaScales, quakeDetail, type Station } from "./detail.ts";
import type { EewArea, ObservationPoint } from "./types.ts";

const stations = new Map<string, Station>([
  ["常総市新石下", { lat: 36.1, lon: 139.9, area: "茨城県南部" }],
  ["筑西市舟生", { lat: 36.3, lon: 139.9, area: "茨城県南部" }],
  ["さいたま中央区下落合", { lat: 35.9, lon: 139.6, area: "埼玉県南部" }],
]);
const pt = (addr: string, scale: number, is_area = false): ObservationPoint => ({ pref: "", addr, is_area, scale });

test("stations become dots and the max per area colors the area", () => {
  const d = quakeDetail([pt("常総市新石下", 30), pt("筑西市舟生", 40), pt("さいたま中央区下落合", 20), pt("未登録の観測点", 50)], stations);
  assert.deepEqual(d.dots, [
    { lat: 36.1, lon: 139.9, scale: 30 },
    { lat: 36.3, lon: 139.9, scale: 40 },
    { lat: 35.9, lon: 139.6, scale: 20 },
  ]);
  assert.deepEqual(d.areas, [
    { name: "茨城県南部", scale: 40 },
    { name: "埼玉県南部", scale: 20 },
  ]);
  assert.deepEqual(d.missing, ["未登録の観測点"]);
});

test("scale prompt reports areas directly", () => {
  const d = quakeDetail([pt("茨城県南部", 40, true), pt("茨城県南部", 30, true)], stations);
  assert.deepEqual(d.areas, [{ name: "茨城県南部", scale: 40 }]);
  assert.deepEqual(d.dots, []);
});

test("eew areas use the upper predicted intensity", () => {
  const a = (name: string, scale_from: number, scale_to: number | null): EewArea => ({ pref: "", name, scale_from, scale_to, arrival_time: null, arrived: false });
  assert.deepEqual(eewAreaScales([a("宮城県北部", 50, 55), a("岩手県沿岸南部", 45, null)]), [
    { name: "宮城県北部", scale: 55 },
    { name: "岩手県沿岸南部", scale: 45 },
  ]);
});
