import { test } from "node:test";
import assert from "node:assert/strict";
import { GroupStore, latestEew, summarizeQuake, type EewGroup, type QuakeGroup } from "./groups.ts";
import type { EewDetectionEvent, EewEvent, QuakeEvent } from "./types.ts";

const ev = (id: string): EewDetectionEvent => ({
  id,
  source: "test",
  received_at_ms: 1,
  kind: "eew_detection",
  detection_type: "Full",
});

test("duplicate ids are ignored", () => {
  const s = new GroupStore();
  assert.ok(s.add(ev("a")));
  assert.equal(s.add(ev("a")), null);
});

test("remembered ids are bounded", () => {
  const s = new GroupStore(2);
  s.add(ev("a"));
  s.add(ev("b"));
  s.add(ev("c")); // a を忘れる
  assert.equal(s.add(ev("c")), null);
  assert.ok(s.add(ev("a")));
});

const quake = (id: string, info_type: QuakeEvent["info_type"], name: string | null, prefs: string[], minute = "2026/09/28 12:00"): QuakeEvent => ({
  id,
  source: "test",
  received_at_ms: 1,
  kind: "quake",
  info_type,
  origin_time: `${minute}:00`,
  origin_time_ms: null,
  issued_at: `${minute}:30`,
  hypocenter: name ? { name, latitude: null, longitude: null, depth_km: null, magnitude: null } : null,
  max_scale: 30,
  domestic_tsunami: "None",
  points: [],
  pref_max: prefs.map((pref) => ({ pref, scale: 30 })),
  comment: "",
});

test("scale prompt without a hypocenter is joined by the later hypocenter report", () => {
  const s = new GroupStore();
  const a = s.add(quake("1", "scale_prompt", null, ["宮城県", "岩手県"]))!;
  const b = s.add(quake("2", "destination", "宮城県沖", []))!;
  const c = s.add(quake("3", "detail_scale", "宮城県沖", ["宮城県"]))!;
  assert.equal(a.key, b.key);
  assert.equal(a.key, c.key);
  assert.equal(s.list().length, 1);
});

test("different earthquakes in the same minute are kept apart", () => {
  const s = new GroupStore();
  const a = s.add(quake("1", "scale_prompt", null, ["宮城県"]))!;
  // 別の場所 (揺れた県が重ならない) の地震は震源のない震度速報に合流しない
  const b = s.add(quake("2", "scale_and_destination", "千葉県東方沖", ["千葉県"]))!;
  const c = s.add(quake("3", "detail_scale", "宮城県沖", ["宮城県"]))!;
  const d = s.add(quake("4", "detail_scale", "千葉県東方沖", ["千葉県", "茨城県"]))!;
  assert.notEqual(a.key, b.key);
  assert.equal(a.key, c.key);
  assert.equal(b.key, d.key);
  assert.equal(s.list().length, 2);
});

test("reports of different minutes are different earthquakes", () => {
  const s = new GroupStore();
  const a = s.add(quake("1", "detail_scale", "宮城県沖", ["宮城県"], "2026/09/28 12:00"))!;
  const b = s.add(quake("2", "detail_scale", "宮城県沖", ["宮城県"], "2026/09/28 12:05"))!;
  assert.notEqual(a.key, b.key);
});

test("the summary of a quake takes each field from the latest report that has it", () => {
  const s = new GroupStore();
  const withPoints = (q: QuakeEvent, scale: number): QuakeEvent => ({
    ...q,
    max_scale: scale,
    points: [{ pref: "宮城県", addr: "仙台市青葉区", is_area: false, scale }],
  });
  s.add(withPoints(quake("1", "scale_prompt", null, ["宮城県"]), 40));
  s.add({ ...quake("2", "destination", "宮城県沖", []), max_scale: -1, comment: "震源情報" });
  const g = s.add(withPoints(quake("3", "detail_scale", "宮城県沖", ["宮城県"]), 50))!;
  assert.equal(g.kind, "quake");
  const q = summarizeQuake(g as QuakeGroup);
  assert.equal(q.infoLabel, "各地の震度に関する情報");
  assert.equal(q.hypocenter?.name, "宮城県沖");
  // 最大震度はまとめた情報の中で最大、観測点は観測点のある最新の報から
  assert.equal(q.maxScale, 50);
  assert.equal(q.points[0].scale, 50);
  assert.equal(q.comment, "");
});

test("the latest eew is the one with the largest serial", () => {
  const s = new GroupStore();
  const e = (id: string, serial: string): EewEvent =>
    ({ id, source: "t", received_at_ms: 1, kind: "eew", event_id: "E", serial, cancelled: false, test: false, warning: true, issued_at: "", origin_time: null, origin_time_ms: null, hypocenter: null, areas: [], pref_max: [], max_scale: 40 }) as EewEvent;
  s.add(e("a", "2"));
  s.add(e("b", "10"));
  const g = s.add(e("c", "9"))!;
  assert.equal(latestEew(g as EewGroup).serial, "10");
});

test("updatedAt follows the received time even when it is negative (records before 1970)", () => {
  const s = new GroupStore();
  const at = (id: string, minute: string, ms: number): QuakeEvent => ({ ...quake(id, "detail_scale", "長野県北部", [], minute), received_at_ms: ms });
  const t1 = Date.UTC(1966, 0, 23);
  const a = s.add(at("1", "1966/01/23 20:15", t1))!;
  const b = s.add(at("2", "1966/01/23 20:16", t1 + 60_000))!;
  assert.equal(a.updatedAt, t1);
  assert.equal(b.updatedAt, t1 + 60_000);
  assert.equal(s.list()[0].key, b.key);
});
