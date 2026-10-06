// サブの地図の見出しと凡例の中身 (詳細パネルと同じ文言を共有する)
import { test } from "node:test";
import assert from "node:assert/strict";
import { eewKindLabel, hypoText, subCaption, subLegend, TSUNAMI_TEXT } from "./sub-caption.ts";
import type { EewGroup, Group, QuakeGroup } from "./groups.ts";
import type { EewArea, EewEvent, Hypocenter, ObservationPoint, QuakeEvent } from "./types.ts";

const hypo = (o: Partial<Hypocenter> = {}): Hypocenter => ({ name: "千葉県北東部", latitude: 35.7, longitude: 140.8, depth_km: 40, magnitude: 5.3, ...o });
const area = (name: string, from: number, to: number | null): EewArea => ({ pref: "千葉県", name, scale_from: from, scale_to: to, arrival_time: null, arrived: false });
const pt = (addr: string, scale: number, is_area = false): ObservationPoint => ({ pref: "千葉県", addr, is_area, scale });

function eew(serial: number, f: Partial<EewEvent> = {}): EewEvent {
  return {
    id: `a-${serial}`, source: "test", received_at_ms: 1000 * serial, kind: "eew", event_id: "a", serial: String(serial),
    cancelled: false, test: false, warning: false, issued_at: "2026/10/01 21:27:00", origin_time: "2026/10/01 21:26:50", origin_time_ms: 1,
    hypocenter: hypo(), areas: [], pref_max: [], max_scale: 40, ...f,
  };
}
const eewGroup = (...events: EewEvent[]): EewGroup => ({ key: "e:a", kind: "eew", updatedAt: 1, events });

function quake(f: Partial<QuakeEvent> = {}): QuakeEvent {
  return {
    id: "q1", source: "test", received_at_ms: 1, kind: "quake", info_type: "detail_scale", origin_time: "2026/10/01 21:26:00", origin_time_ms: 1,
    issued_at: "2026/10/01 21:30:00", hypocenter: hypo(), max_scale: 40, domestic_tsunami: "None", points: [], pref_max: [], comment: "", ...f,
  };
}
const quakeGroup = (...events: QuakeEvent[]): QuakeGroup => ({ key: "quake:q1", kind: "quake", updatedAt: 1, events });

test("EEW forecast: kind with serial, hypocenter facts, time, no tsunami", () => {
  const c = subCaption(eewGroup(eew(5)))!;
  assert.equal(c.scale, 40);
  assert.equal(c.kind, "緊急地震速報 (予報) 第5報");
  assert.equal(c.eew, "forecast");
  assert.equal(c.title, "千葉県北東部");
  assert.equal(c.facts, "M5.3 / 深さ40km");
  assert.equal(c.time, "2026/10/01 21:26:50 発生");
  assert.equal(c.tsunami, null);
});

test("EEW warning and test marks", () => {
  const c = subCaption(eewGroup(eew(2, { warning: true, test: true })))!;
  assert.equal(c.eew, "warning");
  assert.equal(c.kind, "緊急地震速報 (警報) [テスト] 第2報");
  assert.equal(eewKindLabel(eew(2, { warning: true, test: true })), c.kind);
});

test("EEW without origin time falls back to the issue time; without hypocenter is unknown", () => {
  const c = subCaption(eewGroup(eew(1, { origin_time: null, hypocenter: null })))!;
  assert.equal(c.time, "2026/10/01 21:27:00 発生");
  assert.equal(c.title, "震源不明");
  assert.equal(c.facts, "");
});

test("EEW cancelled", () => {
  const c = subCaption(eewGroup(eew(1), eew(2, { cancelled: true })))!;
  assert.equal(c.title, "取り消されました");
});

test("scale prompt without a hypocenter: investigating, no facts, tsunami checking", () => {
  const c = subCaption(quakeGroup(quake({ info_type: "scale_prompt", hypocenter: null, domestic_tsunami: "Checking", max_scale: 30 })))!;
  assert.equal(c.scale, 30);
  assert.equal(c.kind, "震度速報");
  assert.equal(c.eew, null);
  assert.equal(c.title, "震源調査中");
  assert.equal(c.facts, "");
  assert.equal(c.tsunami, "津波の有無を調査中");
});

test("confirmed quake: shallow depth, warning tsunami text, later events fill in", () => {
  const c = subCaption(quakeGroup(quake({ info_type: "scale_prompt", hypocenter: null }), quake({ hypocenter: hypo({ depth_km: 0 }), domestic_tsunami: "Warning" })))!;
  assert.equal(c.kind, "各地の震度に関する情報");
  assert.equal(c.facts, "M5.3 / ごく浅い");
  assert.equal(c.tsunami, TSUNAMI_TEXT.Warning);
  assert.equal(c.tsunami, "津波警報等 発表中");
  assert.equal(c.time, "2026/10/01 21:26:00 発生");
});

test("detail panel helpers: hypoText keeps the same rule", () => {
  assert.equal(hypoText(null), "震源調査中");
  assert.equal(hypoText(hypo({ name: "", magnitude: null, depth_km: null })), "震源不明");
  assert.equal(hypoText(hypo()), "千葉県北東部 / M5.3 / 深さ40km");
});

test("groups other than quake and eew have no caption or legend", () => {
  const t = { key: "tsunami:x", kind: "tsunami", updatedAt: 1, events: [] } as Group;
  assert.equal(subCaption(t), null);
  assert.equal(subLegend(t, false), null);
});

test("legend of a confirmed quake: scales from points and prefectures, descending, no duplicates", () => {
  const g = quakeGroup(quake({ points: [pt("a", 30), pt("b", 40), pt("c", 30), pt("d", 20)], pref_max: [{ pref: "千葉県", scale: 40 }] }));
  const l = subLegend(g, false)!;
  assert.deepEqual(l.scales, [40, 30, 20]);
  assert.deepEqual([l.forecast, l.epicenter, l.waves], [false, true, false]);
  assert.equal(subLegend(g, true)!.waves, true);
});

test("legend of a quake with no hypocenter position has no epicenter", () => {
  const l = subLegend(quakeGroup(quake({ hypocenter: null, points: [pt("a", 30, true)] })), false)!;
  assert.equal(l.epicenter, false);
  assert.deepEqual(l.scales, [30]);
});

test("legend of an EEW: forecast fill from areas and prefectures", () => {
  const g = eewGroup(eew(5, { areas: [area("千葉県北東部", 40, 45), area("茨城県南部", 30, null)], pref_max: [{ pref: "東京都", scale: 20 }] }));
  const l = subLegend(g, true)!;
  assert.deepEqual(l.scales, [45, 30, 20]);
  assert.deepEqual([l.forecast, l.epicenter, l.waves], [true, true, true]);
});

test("legend of an EEW with nothing to fill: no forecast, just the epicenter", () => {
  const l = subLegend(eewGroup(eew(1)), false)!;
  assert.deepEqual(l.scales, []);
  assert.deepEqual([l.forecast, l.epicenter], [false, true]);
});

test("legend of a cancelled EEW is empty", () => {
  const l = subLegend(eewGroup(eew(1), eew(2, { cancelled: true })), true)!;
  assert.deepEqual(l, { scales: [], forecast: false, epicenter: false, waves: false });
});
