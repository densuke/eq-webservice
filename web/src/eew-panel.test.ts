// 緊急地震速報の常設パネルの見せ方 (時刻は実行環境の TZ によらず日本時間)
import { test } from "node:test";
import assert from "node:assert/strict";
import { eewPanelView } from "./eew-panel.ts";
import { scaleLabel } from "./scale.ts";
import type { EewEvent } from "./types.ts";

// 日本時間 2026-10-05 21:27:05
const T = Date.UTC(2026, 9, 5, 12, 27, 5);

function eew(f: Partial<EewEvent> = {}): EewEvent {
  return {
    id: "a-3",
    source: "test",
    received_at_ms: T + 1000,
    kind: "eew",
    event_id: "a",
    serial: "3",
    cancelled: false,
    test: false,
    warning: true,
    issued_at: "2026/10/05 21:27:10",
    origin_time: "2026/10/05 21:27:05",
    origin_time_ms: T,
    hypocenter: { name: "千葉県北東部", latitude: 35.7, longitude: 140.8, depth_km: 50, magnitude: 5.3 },
    areas: [],
    pref_max: [],
    max_scale: 45,
    ...f,
  };
}

test("active EEW: shows the top one with Japan time and more count", () => {
  const v = eewPanelView([eew(), eew({ id: "b-1", event_id: "b", warning: false })], null);
  assert.equal(v.state, "active");
  assert.equal(v.title, "緊急地震速報 (警報)");
  assert.deepEqual(v.rows.map((r) => r.label), ["震源", "発生", "規模", "深さ", "予測最大震度", "報"]);
  assert.deepEqual(v.rows.map((r) => r.value), ["千葉県北東部", "21:27:05", "M5.3", "約50km", scaleLabel(45), "第3報"]);
  assert.equal(v.more, 1);
  assert.equal(v.warning, true);
  assert.equal(v.message, null);
});

test("active EEW with unknowns", () => {
  const v = eewPanelView([eew({ hypocenter: null, origin_time_ms: null })], null);
  assert.deepEqual(v.rows.slice(0, 4).map((r) => r.value), ["調査中", "—", "—", "—"]);
  for (const r of v.rows) assert.doesNotMatch(r.value, /null|NaN|undefined/);
  // 震源はあるが規模・深さが無い
  const w = eewPanelView([eew({ hypocenter: { name: "X", latitude: 0, longitude: 0, depth_km: null, magnitude: null } })], null);
  assert.deepEqual(w.rows.slice(0, 4).map((r) => r.value), ["X", "21:27:05", "—", "—"]);
});

test("test EEW gets the test mark", () => {
  const v = eewPanelView([eew({ test: true, warning: false })], null);
  assert.equal(v.title, "【テスト】緊急地震速報 (予報)");
  assert.equal(v.warning, false);
});

test("no EEW: says none and shows the last one", () => {
  const v = eewPanelView([], eew({ warning: false, max_scale: 40 }));
  assert.equal(v.state, "none");
  assert.equal(v.warning, false);
  assert.equal(v.title, "緊急地震速報");
  assert.equal(v.more, 0);
  assert.equal(v.message, "現在、発表はありません");
  assert.deepEqual(v.rows, [{ label: "最後の発表", value: `10/05 21:27 千葉県北東部 (予報・予測最大震度${scaleLabel(40)})` }]);
});

test("last EEW falls back to received time and unknown hypocenter", () => {
  const v = eewPanelView([], eew({ origin_time_ms: null, hypocenter: null }));
  assert.equal(v.rows[0].value, `10/05 21:27 震源不明 (警報・予測最大震度${scaleLabel(45)})`);
});

test("no EEW and no history", () => {
  const v = eewPanelView([], null);
  assert.deepEqual(v.rows, []);
  assert.equal(v.message, "現在、発表はありません");
});

test("depth 0 km reads as very shallow", () => {
  const v = eewPanelView([eew({ hypocenter: { name: "X", latitude: 0, longitude: 0, depth_km: 0, magnitude: 5 } })], null);
  assert.equal(v.rows.find((r) => r.label === "深さ")?.value, "ごく浅い");
});
