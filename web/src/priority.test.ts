import { test } from "node:test";
import assert from "node:assert/strict";
import { byPriority, sameQuake } from "./priority.ts";

const t0 = Date.UTC(2026, 8, 28, 3, 0, 0);

test("the stronger shaking wins, then the newer one", () => {
  const cs = [
    { key: "a", scale: 30, at: t0 + 60_000 },
    { key: "b", scale: 55, at: t0 },
    { key: "c", scale: 55, at: t0 + 1_000 },
  ];
  assert.deepEqual([...cs].sort(byPriority).map((c) => c.key), ["c", "b", "a"]);
});

test("an eew and a quake report are the same earthquake when origin times are close", () => {
  const miyagi = { originMs: t0 + 12_000, lat: 38.3, lon: 142.0 };
  // 地震情報の発生時刻は分単位、最初の震度速報には震源が無い
  assert.ok(sameQuake(miyagi, { originMs: t0, lat: null, lon: null }));
  assert.ok(sameQuake(miyagi, { originMs: t0, lat: 38.2, lon: 141.9 }));
  assert.ok(!sameQuake(miyagi, { originMs: t0 + 5 * 60_000, lat: null, lon: null }));
  assert.ok(!sameQuake(miyagi, { originMs: null, lat: null, lon: null }));
});

test("close in time but far apart are different earthquakes", () => {
  const miyagi = { originMs: t0 + 12_000, lat: 38.3, lon: 142.0 };
  const chiba = { originMs: t0, lat: 35.7, lon: 140.9 };
  assert.ok(!sameQuake(miyagi, chiba));
});
