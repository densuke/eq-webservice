import { test } from "node:test";
import assert from "node:assert/strict";
import { schedule, shiftJst } from "./demo.ts";
import type { EewEvent, EqEvent, QuakeEvent } from "./types.ts";

test("JST strings shift by milliseconds", () => {
  assert.equal(shiftJst("2026/01/01 12:00:05", 60_000), "2026/01/01 12:01:05");
  assert.equal(shiftJst("2026/12/31 23:59:30", 45_000), "2027/01/01 00:00:15");
  assert.equal(shiftJst("not a time", 1000), "not a time");
});

const eew = (serial: string, issued: string): EewEvent =>
  ({
    id: `e${serial}`,
    source: "p2pquake",
    received_at_ms: 0,
    kind: "eew",
    event_id: "E1",
    serial,
    issued_at: issued,
    origin_time: "2026/01/01 12:00:00",
    origin_time_ms: Date.UTC(2026, 0, 1, 3, 0, 0),
    areas: [],
  }) as unknown as EewEvent;
const quake = (issued: string): QuakeEvent =>
  ({
    id: "q1",
    source: "p2pquake",
    received_at_ms: 0,
    kind: "quake",
    origin_time: "2026/01/01 12:00:00",
    origin_time_ms: Date.UTC(2026, 0, 1, 3, 0, 0),
    issued_at: issued,
  }) as unknown as QuakeEvent;

test("events play from now, gaps keep real time while waves spread and are shortened after, origin stays the same across reports", () => {
  const now = Date.UTC(2026, 8, 29, 0, 0, 0); // 09:00:00 JST
  const s = schedule([eew("1", "2026/01/01 12:00:05"), eew("2", "2026/01/01 12:00:08"), quake("2026/01/01 12:03:05")], now, 1);
  assert.deepEqual(
    s.map((x) => x.at),
    // 発生から 180 秒 (12:03:00) までは波を描くので実時間、その後の 5 秒は詰める上限 (8 秒) 以内なのでそのまま
    [0, 3000, 180_000],
  );
  const [a, b, c] = s.map((x) => x.event);
  // 最初の報が「今」発表されたことになる。発生時刻は 5 秒前
  assert.equal(a.kind === "eew" && a.issued_at, "2026/09/29 09:00:00");
  assert.equal(a.kind === "eew" && a.origin_time_ms, now - 5000);
  // 続報も発生時刻は同じ (同じ地震のまま)
  assert.equal(b.kind === "eew" && b.origin_time_ms, now - 5000);
  assert.equal(c.kind === "quake" && c.origin_time_ms, now - 5000);
  assert.equal(c.kind === "quake" && c.issued_at, "2026/09/29 09:03:00");
});

test("a later quake keeps its origin just before its own reports even when the gap before it is shortened", () => {
  const now = Date.UTC(2026, 8, 29, 0, 0, 0); // 09:00:00 JST
  const later = { ...quake("2026/01/01 12:10:05"), id: "q2", origin_time: "2026/01/01 12:10:00", origin_time_ms: Date.UTC(2026, 0, 1, 3, 10, 0) };
  const s = schedule([eew("1", "2026/01/01 12:00:05"), eew("2", "2026/01/01 12:00:08"), later], now, 1);
  // 最初の地震の波 (12:03:00 まで) と、後の地震の発生から自分の報まで (5 秒) は実時間、残り 7 分は 8 秒に詰める
  assert.deepEqual(
    s.map((x) => x.at),
    [0, 3000, 188_000],
  );
  const q = s[2].event;
  // 発生は自分の最初の報の 5 秒前 (再生中の「今」より未来にならない)
  assert.equal(q.kind === "quake" && q.origin_time_ms, now + 188_000 - 5000);
  assert.equal(q.kind === "quake" && q.origin_time, "2026/09/29 09:03:03");
  // 前の地震の発生時刻はそのまま
  assert.equal(s[0].event.kind === "eew" && s[0].event.origin_time_ms, now - 5000);
});

test("ids and event ids get a per-run suffix and the source becomes demo", () => {
  const [x] = schedule([eew("1", "2026/01/01 12:00:05")], Date.now(), 7);
  assert.equal(x.event.id, "e1#demo7");
  assert.equal(x.event.source, "demo");
  assert.equal(x.event.kind === "eew" && x.event.event_id, "E1#demo7");
});

test("tsunami reports shift only their issued time", () => {
  const t = { id: "t1", source: "p2pquake", received_at_ms: 0, kind: "tsunami", cancelled: false, issued_at: "2026/01/01 12:02:00", areas: [] } as unknown as EqEvent;
  const [x] = schedule([t], Date.UTC(2026, 8, 29, 0, 0, 0), 1);
  assert.equal(x.event.kind === "tsunami" && x.event.issued_at, "2026/09/29 09:00:00");
  assert.equal(x.event.id, "t1#demo1");
});
