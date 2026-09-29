import { test } from "node:test";
import assert from "node:assert/strict";
import { confidenceGrade, latestUserquake, userquakeShown } from "./userquake.ts";
import type { UserquakeEvent } from "./types.ts";

const uq = (started: string, updated: string): UserquakeEvent =>
  ({ id: updated, source: "p2pquake", received_at_ms: 0, kind: "userquake", started_at: started, updated_at: updated, count: 3, confidence: 0.97, areas: [] }) as UserquakeEvent;
// 2026/09/29 10:00:00 JST
const T0 = Date.UTC(2026, 8, 29, 1, 0, 0);

test("the latest evaluation wins", () => {
  const a = uq("2026/09/29 10:00:00.000", "2026/09/29 10:00:05.000");
  const b = uq("2026/09/29 10:00:00.000", "2026/09/29 10:00:09.000");
  assert.equal(latestUserquake(null, a), a);
  assert.equal(latestUserquake(a, b), b);
  assert.equal(latestUserquake(b, a), b);
});

test("reports are shown for two minutes, unless official earthquake information arrived", () => {
  const u = uq("2026/09/29 10:00:00.000", "2026/09/29 10:00:10.000");
  assert.equal(userquakeShown(u, T0 + 20_000, []), true);
  // 最後の更新から 2 分を過ぎたら出さない
  assert.equal(userquakeShown(u, T0 + 10_000 + 2 * 60_000 + 1, []), false);
  // 報告の後に緊急地震速報・地震情報が届いたら、通常の地震の表示に任せる
  assert.equal(userquakeShown(u, T0 + 20_000, [T0 + 15_000]), false);
  // ずっと前の別の地震の情報は関係ない
  assert.equal(userquakeShown(u, T0 + 20_000, [T0 - 10 * 60_000]), true);
  assert.equal(userquakeShown(null, T0, []), false);
});

test("confidence grades follow P2P earthquake information", () => {
  assert.deepEqual([-0.1, 0.1, 0.3, 0.5, 0.7, 0.9].map(confidenceGrade), ["F", "E", "D", "C", "B", "A"]);
});
