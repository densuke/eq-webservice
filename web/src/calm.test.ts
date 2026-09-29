// 地震が起きたときに BGM・バナーなどの平時の楽しみを引っ込め、落ち着いたら戻すか (calmState)。
import { test } from "node:test";
import assert from "node:assert/strict";
import { GroupStore } from "./groups.ts";
import { calmState } from "./quakes.ts";
import { app } from "./state.ts";
import type { DemoState } from "./state.ts";
import type { EewEvent, EqEvent, QuakeEvent, TsunamiEvent } from "./types.ts";

const T = Date.UTC(2026, 8, 29, 5, 0, 0);
const S = 1000;
const hypo = { name: "茨城県南部", latitude: 36.1, longitude: 140.0, depth_km: 50, magnitude: 5.0 };

/** 地震情報 (T に受信、発生は T + originOffset) */
const quake = (scale: number, originOffset = -10 * S): QuakeEvent => ({
  id: `q${scale}${originOffset}`,
  source: "test",
  received_at_ms: T,
  kind: "quake",
  info_type: "scale_and_destination",
  origin_time: "",
  origin_time_ms: T + originOffset,
  issued_at: "",
  hypocenter: hypo,
  max_scale: scale,
  domestic_tsunami: "None",
  points: [],
  pref_max: [],
  comment: "",
});
const eew: EewEvent = {
  id: "e1",
  source: "test",
  received_at_ms: T,
  kind: "eew",
  event_id: "e1",
  serial: "1",
  cancelled: false,
  test: false,
  warning: true,
  issued_at: "",
  origin_time: "",
  origin_time_ms: T - 5 * S,
  hypocenter: hypo,
  areas: [],
  pref_max: [],
  max_scale: 50,
};
const tsunami = (cancelled: boolean): TsunamiEvent => ({
  id: `t${cancelled}`,
  source: "test",
  received_at_ms: T,
  kind: "tsunami",
  cancelled,
  issued_at: "",
  areas: cancelled ? [] : [{ name: "茨城県", grade: "watch", immediate: false, first_height: null, max_height: null }],
});

function world(events: EqEvent[], t: TsunamiEvent | null = null): void {
  app.world = { store: new GroupStore(), tsunami: t, userquake: null };
  app.selectedKey = null;
  app.demo = null;
  app.calmSince = 0;
  events.forEach((e) => app.world.store.add(e));
}

test("nothing happening: entertainment on and the calm (weather warning) view shown", () => {
  world([]);
  assert.deepEqual(calmState(T, false), { quiet: true, calm: true, tsunami: false });
});

test("a quake pauses entertainment until it settles (3 minutes after the last report and after the waves)", () => {
  world([quake(30)]);
  assert.equal(calmState(T + 1 * S, false).quiet, false);
  assert.equal(calmState(T + 120 * S, false).quiet, false);
  assert.equal(calmState(T + 181 * S, false).quiet, true);
  assert.equal(calmState(T + 181 * S, false).calm, true);
});

test("a minor quake (intensity 2 or less) comes back after 1 minute once its waves are done", () => {
  // 発生から 150 秒後に届いた震度1 (波は発生から 180 秒 = 受信の 30 秒後まで)
  world([quake(10, -150 * S)]);
  assert.equal(calmState(T + 20 * S, false).quiet, false);
  assert.equal(calmState(T + 50 * S, false).quiet, false);
  assert.equal(calmState(T + 61 * S, false).quiet, true);
});

test("an earthquake early warning pauses entertainment while it is shown", () => {
  world([eew]);
  assert.equal(calmState(T + 10 * S, false).quiet, false);
  // 緊急地震速報のバナー (3 分) と波 (発生から 180 秒) が終われば戻る
  assert.equal(calmState(T + 181 * S, false).quiet, true);
});

test("a tsunami forecast pauses entertainment until it is cancelled", () => {
  world([], tsunami(false));
  assert.deepEqual(calmState(T + 3600 * S, false), { quiet: false, calm: false, tsunami: true });
  world([], tsunami(true));
  assert.equal(calmState(T + 10 * S, false).quiet, true);
});

test("a report of shaking (userquake) and the demo also pause entertainment", () => {
  world([]);
  assert.equal(calmState(T, true).quiet, false);
  app.demo = { scenarios: [] } as unknown as DemoState;
  assert.equal(calmState(T, false).quiet, false);
});

test("selecting a past quake only hides the weather warnings; entertainment keeps going", () => {
  world([]);
  app.selectedKey = "q:somewhere";
  assert.deepEqual(calmState(T, false), { quiet: true, calm: false, tsunami: false });
});

test("the 「警報・注意報」 button brings everything back right away; a newer report pauses again", () => {
  world([quake(30)]);
  app.calmSince = T + 5 * S;
  assert.equal(calmState(T + 10 * S, false).quiet, true);
  // その後に新しい情報が届けば、また止める
  app.world.store.add({ ...quake(30), id: "q-new", received_at_ms: T + 20 * S });
  assert.equal(calmState(T + 21 * S, false).quiet, false);
});
