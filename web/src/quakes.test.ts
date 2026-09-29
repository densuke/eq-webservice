import { test } from "node:test";
import assert from "node:assert/strict";
import { GroupStore } from "./groups.ts";
import { currentGroup, priorityGroups, updateNumbers, waveSources } from "./quakes.ts";
import { app } from "./state.ts";
import type { EewEvent, EqEvent, QuakeEvent } from "./types.ts";

const t0 = Date.now();
const jst = (ms: number) => {
  const d = new Date(ms + 9 * 3600_000);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getUTCFullYear()}/${p(d.getUTCMonth() + 1)}/${p(d.getUTCDate())} ${p(d.getUTCHours())}:${p(d.getUTCMinutes())}:${p(d.getUTCSeconds())}`;
};
const hypo = (name: string, lat: number, lon: number) => ({ name, latitude: lat, longitude: lon, depth_km: 10, magnitude: 6.5 });

const eew = (id: string, name: string, lat: number, lon: number, scale: number, originOffset = 0): EewEvent => ({
  id,
  source: "test",
  received_at_ms: t0,
  kind: "eew",
  event_id: id,
  serial: "1",
  cancelled: false,
  test: false,
  warning: true,
  issued_at: jst(t0),
  origin_time: jst(t0 - 10_000 + originOffset),
  origin_time_ms: t0 - 10_000 + originOffset,
  hypocenter: hypo(name, lat, lon),
  areas: [],
  pref_max: [],
  max_scale: scale,
});
const quake = (id: string, name: string, lat: number, lon: number, scale: number): QuakeEvent => ({
  id,
  source: "test",
  received_at_ms: t0 + 1000,
  kind: "quake",
  info_type: "scale_and_destination",
  origin_time: jst(t0 - 10_000),
  origin_time_ms: t0 - 10_000,
  issued_at: jst(t0),
  hypocenter: hypo(name, lat, lon),
  max_scale: scale,
  domestic_tsunami: "None",
  points: [],
  pref_max: [],
  comment: "",
});

function world(events: EqEvent[]): void {
  app.world = { store: new GroupStore(), tsunami: null, userquake: null };
  app.selectedKey = null;
  app.numbers = new Map();
  events.forEach((e) => app.world.store.add(e));
}

test("the stronger earthquake comes first; an eew is replaced by its own quake report", () => {
  world([
    eew("miyagi", "宮城県沖", 38.3, 142.0, 50),
    eew("ishikawa", "石川県能登地方", 37.5, 137.2, 60, 6000),
    quake("q-miyagi", "宮城県沖", 38.3, 142.0, 45),
  ]);
  const order = priorityGroups(Date.now()).map((g) => g.key);
  assert.equal(order[0], "e:ishikawa");
  // 宮城の EEW は、同じ地震の地震情報が届いたので地震情報に任せる
  assert.ok(!order.includes("e:miyagi"));
  assert.equal(order.length, 2);
  assert.equal(currentGroup()?.key, "e:ishikawa");
});

test("waves use the eew (precise origin time) rather than the quake report of the same earthquake", () => {
  world([eew("miyagi", "宮城県沖", 38.3, 142.0, 50), quake("q-miyagi", "宮城県沖", 38.3, 142.0, 45), eew("kumamoto", "熊本県熊本地方", 32.7, 130.8, 55, 12000)]);
  const src = waveSources(Date.now());
  assert.deepEqual(
    src.map((s) => s.group.key),
    ["e:kumamoto", "e:miyagi"],
  );
});

test("a selected earthquake is shown instead of the priority one", () => {
  world([eew("miyagi", "宮城県沖", 38.3, 142.0, 50), eew("ishikawa", "石川県能登地方", 37.5, 137.2, 60, 6000)]);
  app.selectedKey = "e:miyagi";
  assert.equal(currentGroup()?.key, "e:miyagi");
});

test("numbers follow the order of occurrence and an eew shares the number with its quake report", () => {
  world([eew("miyagi", "宮城県沖", 38.3, 142.0, 50), eew("ishikawa", "石川県能登地方", 37.5, 137.2, 60, 6000)]);
  assert.equal(updateNumbers(Date.now()), true);
  assert.equal(app.numbers.get("e:miyagi"), 1);
  assert.equal(app.numbers.get("e:ishikawa"), 2);
  app.world.store.add(quake("q-miyagi", "宮城県沖", 38.3, 142.0, 45));
  updateNumbers(Date.now());
  const qkey = [...app.numbers.keys()].find((k) => k.startsWith("q:"))!;
  assert.equal(app.numbers.get(qkey), 1);
  assert.equal(updateNumbers(Date.now()), false);
});
