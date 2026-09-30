import assert from "node:assert/strict";
import { test } from "node:test";
import { rainColor, rangeLabel, showTomorrow, tempLabel, weatherIcon } from "./weather.ts";

test("札は flipSec ごとに今と明日を交互に出す (0 は今だけ)", () => {
  const at = (s: number) => showTomorrow(s * 1000, 20);
  assert.deepEqual([0, 19, 20, 39, 40, 60].map(at), [false, false, true, true, false, true]);
  assert.equal(showTomorrow(25_000, 0), false);
});

test("最高/最低気温の表示", () => {
  assert.equal(rangeLabel(23.6, 17), "24°/17°");
  assert.equal(rangeLabel(23.6, null), "24°/-");
  assert.equal(rangeLabel(null, 9), "-/9°");
  assert.equal(rangeLabel(null, null), "");
});

test("天気コードと文から絵文字を選ぶ", () => {
  assert.equal(weatherIcon("100", "晴れ", false), "☀️");
  assert.equal(weatherIcon("100", "晴れ", true), "🌙");
  assert.equal(weatherIcon("101", "晴れ時々くもり", false), "🌤️");
  assert.equal(weatherIcon("102", "晴れ一時雨", false), "🌦️");
  assert.equal(weatherIcon("200", "くもり", false), "☁️");
  assert.equal(weatherIcon("211", "くもり夜晴れ", false), "⛅");
  assert.equal(weatherIcon("203", "くもり時々雨", false), "🌧️");
  assert.equal(weatherIcon("300", "雨", false), "☔");
  assert.equal(weatherIcon("313", "雨昼過ぎからくもり所により夕方まで雷を伴う", false), "⛈️");
  assert.equal(weatherIcon("400", "雪", false), "❄️");
  assert.equal(weatherIcon("", "", false), "");
});

test("降水量の段階で色を変える", () => {
  assert.equal(rainColor(0.5), "#f2f2ff");
  assert.equal(rainColor(1), "#a0d2ff");
  assert.equal(rainColor(12), "#0041ff");
  assert.equal(rainColor(80), "#b40068");
});

test("気温は整数に丸める", () => {
  assert.equal(tempLabel(22.5), "23°");
  assert.equal(tempLabel(-0.4), "0°");
  assert.equal(tempLabel(null), "");
});
