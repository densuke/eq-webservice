import { test } from "node:test";
import assert from "node:assert/strict";
import { makePlan, shiftJst } from "./demo.ts";
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

const at = (y: number, mo: number, d: number, h: number, mi: number, se: number) => Date.UTC(y, mo - 1, d, h - 9, mi, se);

test("playback starts a few seconds before the quake; gaps keep real time while waves spread and are shortened after", () => {
  const p = makePlan([eew("1", "2026/01/01 12:00:05"), eew("2", "2026/01/01 12:00:08"), quake("2026/01/01 12:03:05")], 1);
  // 再生位置 0 は発生 (12:00:00) の 5 秒前。発生から 180 秒 (12:03:00) までは実時間、その後の 5 秒は上限 (8 秒) 以内なのでそのまま
  assert.deepEqual(
    p.events.map((x) => x.at),
    [10_000, 13_000, 190_000],
  );
  assert.equal(p.end, 190_000);
  // 記録の場面は時刻をずらさない (時計は当時の日時)
  assert.equal(p.toReal(0), at(2026, 1, 1, 11, 59, 55));
  assert.equal(p.toReal(190_000), at(2026, 1, 1, 12, 3, 5));
  const [a, , c] = p.events.map((x) => x.event);
  assert.equal(a.kind === "eew" && a.issued_at, "2026/01/01 12:00:05");
  assert.equal(c.kind === "quake" && c.origin_time_ms, at(2026, 1, 1, 12, 0, 0));
});

test("a made-up scene is moved so that playback starts now, keeping every interval", () => {
  const now = Date.UTC(2026, 8, 29, 0, 0, 0); // 09:00:00 JST
  const p = makePlan([eew("1", "2026/01/01 12:00:05"), quake("2026/01/01 12:03:05")], 1, now);
  assert.equal(p.toReal(0), now);
  const [a, b] = p.events.map((x) => x.event);
  assert.equal(a.kind === "eew" && a.issued_at, "2026/09/29 09:00:10");
  assert.equal(a.kind === "eew" && a.origin_time_ms, now + 5000);
  assert.equal(b.kind === "quake" && b.origin_time_ms, now + 5000);
  assert.equal(b.kind === "quake" && b.issued_at, "2026/09/29 09:03:10");
});

test("a long quiet gap is shortened and the clock runs fast through it, a later quake keeps its own timing", () => {
  const later = { ...quake("2026/01/01 12:10:05"), id: "q2", origin_time: "2026/01/01 12:10:00", origin_time_ms: at(2026, 1, 1, 12, 10, 0) };
  const p = makePlan([eew("1", "2026/01/01 12:00:05"), eew("2", "2026/01/01 12:00:08"), later], 1);
  // 最初の地震の波 (12:03:00 まで) は実時間、何も無い 7 分は 8 秒に詰め、後の地震の発生から報まで (5 秒) は実時間
  assert.deepEqual(
    p.events.map((x) => x.at),
    [10_000, 13_000, 198_000],
  );
  assert.equal(p.toReal(193_000), at(2026, 1, 1, 12, 10, 0));
  // 詰めた間は時計が早く進む (8 秒で 7 分)
  assert.equal(p.toReal(189_000), at(2026, 1, 1, 12, 6, 30));
  // 位置 -> 時刻は増え続ける
  const xs = [0, 5000, 100_000, 186_000, 190_000, 194_000, 250_000].map((x) => p.toReal(x));
  assert.deepEqual(xs, [...xs].sort((u, v) => u - v));
});

test("ids and event ids get a per-run suffix and the source becomes demo", () => {
  const [x] = makePlan([eew("1", "2026/01/01 12:00:05")], 7).events;
  assert.equal(x.event.id, "e1#demo7");
  assert.equal(x.event.source, "demo");
  assert.equal(x.event.kind === "eew" && x.event.event_id, "E1#demo7");
});

test("tsunami reports are moved like the rest", () => {
  const t = { id: "t1", source: "p2pquake", received_at_ms: 0, kind: "tsunami", cancelled: false, issued_at: "2026/01/01 12:02:00", areas: [] } as unknown as EqEvent;
  const [x] = makePlan([t], 1, Date.UTC(2026, 8, 29, 0, 0, 0)).events;
  // 地震が無いので、最初の報の 5 秒前から
  assert.equal(x.at, 5000);
  assert.equal(x.event.kind === "tsunami" && x.event.issued_at, "2026/09/29 09:00:05");
  assert.equal(x.event.id, "t1#demo1");
});
