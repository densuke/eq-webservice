import { test } from "node:test";
import assert from "node:assert/strict";
import { makePlan } from "./demo.ts";
import { GroupStore } from "./groups.ts";
import { HISTORY_LEAD_MS, gatherEvents, hindsightOf, historyStart, sameQuakeEvents, skipRanges } from "./history.ts";
import { groupPlace } from "./quakes.ts";
import { app } from "./state.ts";
import type { EewEvent, EqEvent, QuakeEvent } from "./types.ts";

// 2026-09-30 14:00:37 JST の地震 (地震情報の発生時刻は分単位で 14:00)
const at = (h: number, m: number, s: number) => Date.UTC(2026, 8, 30, h - 9, m, s);
const jst = (h: number, m: number, s: number) => `2026/09/30 ${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
const hypo = (lat: number, lon: number) => ({ name: "x", latitude: lat, longitude: lon, depth_km: 10, magnitude: 4 });

const eew = (id: string, serial: string, recv: number, origin = at(14, 0, 37), lat = 24.4, lon = 123): EewEvent =>
  ({
    id,
    source: "wolfx",
    received_at_ms: recv,
    kind: "eew",
    event_id: "E1",
    serial,
    cancelled: false,
    test: false,
    warning: false,
    issued_at: jst(14, 0, 50),
    origin_time: jst(14, 0, 37),
    origin_time_ms: origin,
    hypocenter: hypo(lat, lon),
    areas: [],
    pref_max: [],
    max_scale: 30,
  }) as EewEvent;
const quake = (id: string, recv: number, minute = 0, lat = 24.4, lon = 123): QuakeEvent =>
  ({
    id,
    source: "p2pquake",
    received_at_ms: recv,
    kind: "quake",
    info_type: "scale_and_destination",
    origin_time: jst(14, minute, 0),
    origin_time_ms: at(14, minute, 0),
    issued_at: jst(14, 5, 36),
    hypocenter: hypo(lat, lon),
    max_scale: 30,
    domestic_tsunami: "None",
    points: [],
    pref_max: [],
    comment: "",
  }) as QuakeEvent;

const mine = [eew("e2", "2", at(14, 0, 55)), eew("e1", "1", at(14, 0, 50)), quake("q1", at(14, 5, 36))];
const other = [{ ...eew("far", "1", at(14, 0, 52), at(14, 0, 40), 35, 139), event_id: "E9" }, quake("later", at(14, 30, 0), 30)];
const target = groupPlace(new GroupStore().add(mine[0])!);

test("only events of the same earthquake are kept, in the order they arrived", () => {
  const got = sameQuakeEvents([...other, ...mine, { id: "t", source: "x", received_at_ms: 1, kind: "tsunami" } as EqEvent], target);
  assert.deepEqual(
    got.map((e) => e.id),
    ["e1", "e2", "q1"],
  );
});

test("playback starts 10 seconds before the second-resolution origin of the eew, not the minute-resolution one of the quake", () => {
  assert.equal(historyStart(mine), at(14, 0, 37) - HISTORY_LEAD_MS);
  assert.equal(historyStart([]), null);
});

test("without an eew, playback starts 10 seconds before the first report arrived", () => {
  const events = [quake("q2", at(14, 3, 20)), quake("q1", at(14, 1, 43))];
  assert.equal(historyStart(events), at(14, 1, 43) - HISTORY_LEAD_MS);
});

test("gaps longer than 20 seconds are skipped from 5 seconds after a report to 5 seconds before the next; exactly 20 is not", () => {
  assert.deepEqual(skipRanges([0, 20_000, 40_001, 50_000]), [{ from: 25_000, to: 35_001 }]);
  assert.deepEqual(skipRanges([0, 20_000]), []);
  assert.deepEqual(skipRanges([]), []);
});

test("the plan starts at that time, and each report arrives at its issued time", () => {
  const start = historyStart(mine)!;
  const p = makePlan(sameQuakeEvents(mine, target), 1, undefined, start);
  assert.equal(p.toReal(0), start);
  // 14:00:27 から 14:00:50 (第 1 報) までは実時間 (23 秒)
  assert.equal(p.events[0].at, 23_000);
  assert.equal(p.toReal(p.events[0].at), at(14, 0, 50));
});

const store = (events: EqEvent[]) => {
  app.world = { store: new GroupStore(), tsunami: null, userquake: null };
  events.forEach((e) => app.world.store.add(e));
};

test("the archive is used when it has the earthquake", async () => {
  store([]);
  let range: [number, number] | null = null;
  const got = await gatherEvents(new GroupStore().add(mine[0])!, async (from, to) => ((range = [from, to]), [...other, ...mine]));
  assert.deepEqual(
    got.map((e) => e.id),
    ["e1", "e2", "q1"],
  );
  assert.deepEqual(range, [at(14, 0, 37) - 60_000, at(14, 0, 37) + 15 * 60_000]);
});

test("the events the browser already holds are used when the archive is unavailable or does not have it", async () => {
  store(mine);
  const g = app.world.store.list()[0];
  for (const fetchArchive of [async () => null, async () => [], async () => other, async () => Promise.reject(new Error("net"))]) {
    const got = await gatherEvents(g, fetchArchive);
    assert.deepEqual(
      got.map((e) => e.id),
      ["e1", "e2", "q1"],
    );
  }
});

test("the hindsight epicenter prefers the detailed intensities, then the epicenter report, then the last eew", () => {
  const q = (id: string, recv: number, type: QuakeEvent["info_type"], lat: number) => ({ ...quake(id, recv, 0, lat, 140), info_type: type });
  const [a, b, c] = [q("a", 3, "detail_scale", 36.1), q("b", 2, "destination", 36.2), q("c", 1, "scale_prompt", 36.3)];
  const e = eew("e", "5", 0, at(14, 0, 37), 36.4, 140);
  assert.equal(hindsightOf([e, c, b, a])?.lat, 36.1);
  assert.equal(hindsightOf([e, c, b])?.lat, 36.2);
  // 震度速報の震源は使わない
  assert.equal(hindsightOf([e, c])?.lat, 36.4);
  assert.equal(hindsightOf([{ ...c, hypocenter: null }]), null);
  assert.equal(hindsightOf([]), null);
  // 最終報 (後に届いた方)
  assert.equal(hindsightOf([eew("e1", "1", 1, at(14, 0, 37), 36.5), eew("e2", "2", 2, at(14, 0, 37), 36.6)])?.lat, 36.6);
});

test("the hindsight origin is the eew second when there is one, else the minute of the quake report", () => {
  assert.equal(hindsightOf([quake("q", 5, 0), eew("e", "1", 1)])?.originMs, at(14, 0, 37));
  assert.equal(hindsightOf([quake("q", 5, 0)])?.originMs, at(14, 0, 0));
  assert.deepEqual(hindsightOf([quake("q", 5, 0, 24.4, 123)]), { lat: 24.4, lon: 123, depth: 10, originMs: at(14, 0, 0) });
});
