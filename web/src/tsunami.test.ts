import { test } from "node:test";
import assert from "node:assert/strict";
import { activeAreas, latestTsunami, tsunamiAlert } from "./tsunami.ts";
import type { TsunamiArea, TsunamiEvent } from "./types.ts";

const area = (name: string, grade: TsunamiArea["grade"]) => ({ name, grade, immediate: false, first_height: null, max_height: null });
const ev = (issued_at: string, areas: TsunamiArea[], cancelled = false) => ({ kind: "tsunami", issued_at, areas, cancelled }) as TsunamiEvent;

test("newer forecast replaces, cancel clears, late older one is ignored", () => {
  const a = ev("2026/09/28 12:00:00", [area("宮城県", "watch")]);
  const b = ev("2026/09/28 12:10:00", [area("宮城県", "warning")]);
  const c = ev("2026/09/28 13:00:00", [], true);
  assert.equal(latestTsunami(null, a), a);
  assert.equal(latestTsunami(a, b), b);
  assert.equal(latestTsunami(b, a), b);
  assert.deepEqual(activeAreas(latestTsunami(b, c)), []);
  // 解除のあとに古い予報が遅れて届いても復活しない
  assert.deepEqual(activeAreas(latestTsunami(c, b)), []);
});

test("sounds only when a forecast appears or its grade rises", () => {
  const watch = [area("宮城県", "watch")];
  const warning = [area("宮城県", "warning"), area("岩手県", "watch")];
  assert.equal(tsunamiAlert([], watch), "medium");
  assert.equal(tsunamiAlert(watch, watch), null);
  assert.equal(tsunamiAlert(watch, warning), "strong");
  assert.equal(tsunamiAlert(warning, watch), null);
  assert.equal(tsunamiAlert([], [area("宮城県", "major_warning")]), "strong");
  assert.equal(tsunamiAlert(watch, []), null);
});
