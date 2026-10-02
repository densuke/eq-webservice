import { test } from "node:test";
import assert from "node:assert/strict";
import { GroupStore } from "./groups.ts";
import { currentGroup, pendingHindsight, priorityGroups, shakenGeo, updateNumbers, waveSources } from "./quakes.ts";
import { DEFAULT_STOP_KM, stopRadiusKm } from "./camera.ts";
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
    src.map((s) => s.group?.key),
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

test("a hindsight epicenter is shown until an epicenter of the same earthquake arrives", () => {
  const h = { lat: 36.2, lon: 140.1, depth: 50, originMs: t0 - 10_000 };
  const store = new GroupStore();
  assert.equal(pendingHindsight(null, []), null);
  assert.equal(pendingHindsight(h, store.list()), h);
  // 震度速報 (震源なし) では、まだ出す
  const prompt = { ...quake("q1", "", 0, 0, 40), info_type: "scale_prompt" as const, hypocenter: null };
  store.add(prompt);
  assert.equal(pendingHindsight(h, store.list()), h);
  // 別の場所の地震の震源では消さない
  store.add(eew("other", "宮城県沖", 38.3, 142.0, 50));
  assert.equal(pendingHindsight(h, store.list()), h);
  // 同じ地震の震源が届いたら消す
  store.add(quake("q2", "茨城県南部", 36.1, 140.0, 40));
  assert.equal(pendingHindsight(h, store.list()), null);
});

test("waves start when the epicenter arrives after the intensity report, from the origin time", () => {
  const prompt = { ...quake("q1", "", 0, 0, 40), info_type: "scale_prompt" as const, hypocenter: null };
  world([prompt]);
  assert.equal(waveSources(t0).length, 0);
  // 震源の情報が届く: 発生時刻 (分単位) を起点に、その時点の半径から描く
  app.world.store.add(quake("q2", "茨城県南部", 36.1, 140.0, 40));
  const [src] = waveSources(t0 + 60_000);
  assert.equal(src.origin, t0 - 10_000);
  assert.equal(src.group?.kind, "quake");
  // 発生から WAVE_MAX_SEC を過ぎていれば描かない
  assert.equal(waveSources(t0 + 200_000).length, 0);
});

test("a hindsight epicenter draws waves from its origin, and not before the origin", () => {
  world([]);
  app.demo = { hindsight: { lat: 36.2, lon: 140.1, depth: 50, originMs: t0 } } as typeof app.demo;
  try {
    assert.equal(waveSources(t0 - 1000).length, 0);
    const [src] = waveSources(t0 + 5000);
    assert.deepEqual([src.lat, src.lon, src.depth, src.origin, src.group], [36.2, 140.1, 50, t0, undefined]);
    // 本物の震源が届いたら、そちらの波だけ
    app.world.store.add(quake("q2", "茨城県南部", 36.2, 140.1, 40));
    assert.equal(waveSources(t0 + 5000).length, 1);
    assert.equal(waveSources(t0 + 5000)[0].group?.kind, "quake");
  } finally {
    app.demo = null;
  }
});

// 2026-10-02 熊本県熊本地方: EEW (震度 3 の予想) は区域を持たず、のちの地震情報 (震度速報) が熊本の区域を持つ。
// 波を描いている間 (180 秒まで) のカメラの目標は EEW 側だが、揺れた範囲は同じ地震の報すべての和で見る
test("the shaken area of an eew without areas comes from the quake report of the same earthquake", () => {
  const e = { ...eew("kumamoto", "熊本県熊本地方", 32.8, 130.7, 30, 10_000), received_at_ms: t0 };
  world([e]);
  const g = app.world.store.list()[0];
  // 震度速報が届く前: 揺れた範囲は分からず、止める半径は既定のまま
  assert.deepEqual(shakenGeo(g), { prefs: [], areas: [] });
  assert.equal(stopRadiusKm(0, 0, null), DEFAULT_STOP_KM);
  // 震度速報 (熊本県熊本の区域) が届くと、EEW の群からも熊本の区域が見える
  const q: QuakeEvent = {
    ...quake("q1", "熊本県熊本地方", 32.8, 130.7, 30),
    info_type: "scale_prompt",
    origin_time_ms: e.origin_time_ms,
    points: [{ pref: "熊本県", addr: "熊本県熊本", is_area: true, scale: 30 }],
    pref_max: [{ pref: "熊本県", scale: 30 }],
  };
  app.world.store.add(q);
  const eewGroup = app.world.store.list().find((x) => x.kind === "eew")!;
  assert.deepEqual(shakenGeo(eewGroup), { prefs: ["熊本県"], areas: ["熊本県熊本"] });
  // 別の地震の報は混ぜない
  app.world.store.add({ ...quake("q2", "北海道", 43, 143, 40), origin_time_ms: t0 - 3_600_000, points: [{ pref: "北海道", addr: "北海道十勝", is_area: true, scale: 40 }] });
  assert.deepEqual(shakenGeo(eewGroup).areas, ["熊本県熊本"]);
});
