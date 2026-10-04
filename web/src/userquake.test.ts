import { test } from "node:test";
import assert from "node:assert/strict";
import { confidenceGrade, latestUserquake, userquakeCredible, userquakeReadable, userquakeShown } from "./userquake.ts";
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

const area = (code: number, confidence: number) => ({ code, count: 3, confidence });
const credible = (over: Partial<UserquakeEvent> = {}): UserquakeEvent => ({ ...uq("2026/09/29 10:00:00.000", "2026/09/29 10:00:10.000"), confidence: 0.97, areas: [area(205, 0.9)], ...over });

test("credible needs overall confidence and an A/B area", () => {
  assert.equal(userquakeCredible(credible()), true);
  assert.equal(userquakeCredible(credible({ confidence: 0.5 })), false);
  assert.equal(userquakeCredible(credible({ confidence: 0 })), false);
  assert.equal(userquakeCredible(credible({ areas: [area(205, 0.59)] })), false);
  assert.equal(userquakeCredible(credible({ areas: [] })), false);
  assert.equal(userquakeCredible(credible({ areas: [area(205, 0.3), area(215, 0.6)] })), true);
});

test("readable: suppressed by an official report that arrived since", () => {
  const u = credible();
  assert.equal(userquakeReadable(u, T0 + 20_000, [], null), true);
  assert.equal(userquakeReadable(u, T0 + 20_000, [T0 + 15_000], null), false);
  assert.equal(userquakeReadable(u, T0 + 20_000, [T0 - 10 * 60_000], null), true);
  // 2 分を過ぎた古い評価は読まない
  assert.equal(userquakeReadable(u, T0 + 10_000 + 2 * 60_000 + 1, [], null), false);
  assert.equal(userquakeReadable(credible({ confidence: 0.5 }), T0 + 20_000, [], null), false);
});

test("readable: once per shaking and at most once per ten minutes", () => {
  const u = credible();
  // 同じ揺れ (started_at) は読み直さない
  assert.equal(userquakeReadable(u, T0 + 40_000, [], { startedAt: u.started_at, at: T0 + 20_000 }), false);
  // 別の揺れでも 10 分以内は読まない
  const next = credible({ started_at: "2026/09/29 10:05:00.000", updated_at: "2026/09/29 10:05:10.000" });
  const last = { startedAt: u.started_at, at: T0 + 20_000 };
  assert.equal(userquakeReadable(next, T0 + 5 * 60_000 + 20_000, [], last), false);
  assert.equal(userquakeReadable(next, T0 + 5 * 60_000 + 20_000, [], last, false), true);
  const later = credible({ started_at: "2026/09/29 10:11:00.000", updated_at: "2026/09/29 10:11:10.000" });
  assert.equal(userquakeReadable(later, T0 + 11 * 60_000 + 20_000, [], last), true);
});
