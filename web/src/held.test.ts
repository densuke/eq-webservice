// 緊急地震速報の予想を、同じ地震の間は最大で持ち続ける (2026-10-01 21:27 千葉県北東部の報の並び)
import { test } from "node:test";
import assert from "node:assert/strict";
import { heldForecast } from "./groups.ts";
import { eewAreaScales } from "./detail.ts";
import type { EewArea, EewEvent } from "./types.ts";

const area = (pref: string, name: string, from: number, to: number | null): EewArea => ({ pref, name, scale_from: from, scale_to: to, arrival_time: null, arrived: false });
const CHIBA = (a: number, b: number | null) => area("千葉県", "千葉県北東部", a, b);
const IBARAKI = (a: number, b: number | null) => area("茨城県", "茨城県南部", a, b);

function eew(serial: number, max: number, areas: EewArea[], prefs: [string, number][], f: Partial<EewEvent> = {}): EewEvent {
  return {
    id: `a-${serial}`,
    source: "test",
    received_at_ms: 1000 * serial,
    kind: "eew",
    event_id: "a",
    serial: String(serial),
    cancelled: false,
    test: false,
    warning: false,
    issued_at: "2026/10/01 21:27:00",
    origin_time: "2026/10/01 21:26:50",
    origin_time_ms: 1,
    hypocenter: { name: `s${serial}`, latitude: 35.7, longitude: 140.8, depth_km: 40, magnitude: 4.5 },
    areas,
    pref_max: prefs.map(([pref, scale]) => ({ pref, scale })),
    max_scale: max,
    ...f,
  };
}

// 観測された並びの一部: 第 5 報で地域が出て、第 6・7 報で空になり、第 8 報でまた出て、第 10 報でまた空になる
const seq = [
  eew(1, 30, [], []),
  eew(5, 40, [CHIBA(40, 40), IBARAKI(30, 40)], [["千葉県", 40], ["茨城県", 40]]),
  eew(6, 30, [], []),
  eew(7, 30, [], []),
  eew(8, 40, [CHIBA(40, 40), IBARAKI(30, 40)], [["千葉県", 40], ["茨城県", 40]]),
  eew(9, 40, [CHIBA(40, 40)], [["千葉県", 40]]),
  eew(10, 30, [], []),
];

test("areas and prefectures stay at their maximum after a report that omits them", () => {
  for (const n of [3, 4, 7]) {
    const h = heldForecast(seq.slice(0, n));
    assert.equal(h.max_scale, 40, `max after ${n} reports`);
    assert.deepEqual(eewAreaScales(h.areas).map((a) => [a.name, a.scale]).sort(), [["千葉県北東部", 40], ["茨城県南部", 40]]);
    assert.deepEqual(h.pref_max.map((p) => [p.pref, p.scale]).sort(), [["千葉県", 40], ["茨城県", 40]]);
  }
  assert.deepEqual(heldForecast(seq.slice(0, 1)).areas, []);
});

test("the hypocenter and serial come from the latest report, whatever order they arrive in", () => {
  const h = heldForecast([seq[6], seq[1], seq[3]]);
  assert.equal(h.serial, "10");
  assert.equal(h.hypocenter?.name, "s10");
  assert.equal(h.max_scale, 40);
});

test("an open-ended upper bound does not erase a larger value held from another report", () => {
  const h = heldForecast([eew(1, 40, [IBARAKI(30, 40)], []), eew(2, 30, [IBARAKI(30, null)], [])]);
  assert.deepEqual(eewAreaScales(h.areas), [{ name: "茨城県南部", scale: 40 }]);
});

test("a warning stays a warning", () => {
  const h = heldForecast([eew(1, 40, [], [], { warning: true }), eew(2, 30, [], [])]);
  assert.equal(h.warning, true);
  assert.equal(heldForecast([eew(1, 40, [], []), eew(2, 30, [], [])]).warning, false);
});

test("a cancelled latest report clears the event", () => {
  const h = heldForecast([seq[1], eew(6, 0, [], [], { cancelled: true })]);
  assert.equal(h.cancelled, true);
  assert.equal(h.max_scale, 0);
  assert.deepEqual(h.areas, []);
});
