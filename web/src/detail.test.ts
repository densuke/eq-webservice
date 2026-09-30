import { test } from "node:test";
import assert from "node:assert/strict";
import { droppedForecast, eewAreaScales, forecastTag, keepForecast, overlayForecast, quakeDetail, type Station } from "./detail.ts";
import type { EewArea, EewEvent, ObservationPoint } from "./types.ts";

const stations = new Map<string, Station>([
  ["常総市新石下", { lat: 36.1, lon: 139.9, area: "茨城県南部" }],
  ["筑西市舟生", { lat: 36.3, lon: 139.9, area: "茨城県南部" }],
  ["さいたま中央区下落合", { lat: 35.9, lon: 139.6, area: "埼玉県南部" }],
]);
const pt = (addr: string, scale: number, is_area = false): ObservationPoint => ({ pref: "", addr, is_area, scale });

test("stations become dots and the max per area colors the area", () => {
  const d = quakeDetail([pt("常総市新石下", 30), pt("筑西市舟生", 40), pt("さいたま中央区下落合", 20), pt("未登録の観測点", 50)], stations);
  assert.deepEqual(d.dots, [
    { name: "常総市新石下", lat: 36.1, lon: 139.9, scale: 30 },
    { name: "筑西市舟生", lat: 36.3, lon: 139.9, scale: 40 },
    { name: "さいたま中央区下落合", lat: 35.9, lon: 139.6, scale: 20 },
  ]);
  assert.deepEqual(d.areas, [
    { name: "茨城県南部", scale: 40 },
    { name: "埼玉県南部", scale: 20 },
  ]);
  assert.deepEqual(d.missing, ["未登録の観測点"]);
});

test("a point that carries its own position (old stations in past records) is drawn without the station list", () => {
  const old: ObservationPoint = { ...pt("栗原市築館（旧）＊", 70), station: { lat: 38.73, lon: 141.02, area: "宮城県北部" } };
  const d = quakeDetail([old], stations);
  assert.deepEqual(d.dots, [{ name: "栗原市築館（旧）＊", lat: 38.73, lon: 141.02, scale: 70 }]);
  assert.deepEqual(d.areas, [{ name: "宮城県北部", scale: 70 }]);
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

test("observed intensities overlay the forecast: forecast stays only where nothing is observed yet", () => {
  const observed = [{ name: "石川県能登", scale: 60 }];
  const forecast = [
    { name: "石川県能登", scale: 70 },
    { name: "富山県西部", scale: 50 },
  ];
  assert.deepEqual(overlayForecast(observed, forecast), [
    { name: "石川県能登", scale: 60, forecast: false },
    { name: "富山県西部", scale: 50, forecast: true },
  ]);
  assert.deepEqual(overlayForecast(observed, []), [{ name: "石川県能登", scale: 60, forecast: false }]);
});

test("areas dropped by a later report stay for a while with their last forecast, then go", () => {
  const reports = [
    { at: 0, items: [{ name: "石川県能登", scale: 50 }, { name: "新潟県上越", scale: 40 }] },
    { at: 1000, items: [{ name: "石川県能登", scale: 55 }, { name: "新潟県上越", scale: 45 }, { name: "富山県西部", scale: 40 }] },
    { at: 2000, items: [{ name: "石川県能登", scale: 55 }] },
  ];
  // 2 報目の後、3 報目で外された地域 (最後の予測の震度で)
  assert.deepEqual(droppedForecast(reports, 5000, 8000), [
    { name: "新潟県上越", scale: 45 },
    { name: "富山県西部", scale: 40 },
  ]);
  // 外されてから 8 秒を過ぎたら出さない
  assert.deepEqual(droppedForecast(reports, 10_001, 8000), []);
  // 最後の報にある地域・報が 1 つだけのときは無い
  assert.deepEqual(droppedForecast(reports.slice(0, 1), 0, 8000), []);
});

test("the forecast stays until observed intensities arrive, even after the warning ends", () => {
  // 速報が続いている間は残す
  assert.equal(keepForecast(true, true, 0), true);
  // 速報が終わっても、震源の情報だけ (観測の震度なし) なら残す (2011 年: 14:49 の震源、16:00 の各地の震度)
  assert.equal(keepForecast(false, false, 5 * 60_000), true);
  // 観測の震度が届いたら、速報が終わった後は残さない
  assert.equal(keepForecast(false, true, 60_000), false);
  // 観測が無くても上限 (1 時間) を過ぎたら残さない
  assert.equal(keepForecast(false, false, 60 * 60_000 + 1), false);
});

test("the forecast tag is shown only when there is nothing painted, and follows the latest report", () => {
  const e = { cancelled: false, max_scale: 30, areas: [], pref_max: [] } as unknown as EewEvent;
  assert.equal(forecastTag(e), "予測最大震度3");
  assert.equal(forecastTag({ ...e, max_scale: 20 }), "予測最大震度2");
  assert.equal(forecastTag({ ...e, cancelled: true }), null);
  assert.equal(forecastTag({ ...e, max_scale: -1 }), null);
  assert.equal(forecastTag({ ...e, pref_max: [{ pref: "沖縄県", scale: 30 }] }), null);
  assert.equal(forecastTag({ ...e, areas: [{}] as EewArea[] }), null);
});
