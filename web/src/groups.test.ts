import { test } from "node:test";
import assert from "node:assert/strict";
import { GroupStore } from "./groups.ts";
import type { EewDetectionEvent, QuakeEvent } from "./types.ts";

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
