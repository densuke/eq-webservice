import { test } from "node:test";
import assert from "node:assert/strict";
import { MINOR_SETTLE_MS, SETTLE_MS, byPriority, sameQuake, settleMs } from "./priority.ts";

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

test("minor quakes (intensity 2 or less) go back to the calm view sooner", () => {
  assert.equal(settleMs(10), MINOR_SETTLE_MS);
  assert.equal(settleMs(20), MINOR_SETTLE_MS);
  assert.equal(settleMs(30), SETTLE_MS);
  assert.equal(settleMs(70), SETTLE_MS);
  // 最大震度が分からない (震源の情報だけ・EEW の予測なし) ものは通常どおり
  assert.equal(settleMs(-1), SETTLE_MS);
});
