import assert from "node:assert/strict";
import { test } from "node:test";
import { rainColor, rangeLabel, tempLabel, weatherCaption, weatherIcon, weatherView } from "./weather.ts";

test("種類は flipSec ごとに並び (今, 明日) を順に回す (0 は先頭のまま)", () => {
  const at = (s: number) => weatherView(s * 1000, 20);
  assert.deepEqual([0, 19, 20, 39, 40, 60].map(at), ["now", "now", "tomorrow", "tomorrow", "now", "tomorrow"]);
  assert.equal(weatherView(25_000, 0), "now");
});

test("案内の文は種類から決め、明日は日本時間の明日の日付と曜日を添える", () => {
  const jst = (s: string) => Date.parse(`${s}+09:00`);
  assert.equal(weatherCaption("now", 0), "現在の天気");
  assert.equal(weatherCaption("tomorrow", jst("2026-10-01T09:38:00")), "明日 10/2 (金) の天気");
  // 日本時間では翌日の 0 時を過ぎている (UTC ではまだ 10/1)
  assert.equal(weatherCaption("tomorrow", jst("2026-10-02T01:00:00")), "明日 10/3 (土) の天気");
  assert.equal(weatherCaption("tomorrow", jst("2026-12-31T23:59:00")), "明日 1/1 (金) の天気");
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
