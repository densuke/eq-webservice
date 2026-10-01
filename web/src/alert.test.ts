import { test } from "node:test";
import assert from "node:assert/strict";
import { alertLevel } from "./alert.ts";
import type { EewEvent, QuakeEvent } from "./types.ts";

const quake = (max_scale: number) => ({ kind: "quake", max_scale }) as QuakeEvent;
const eew = (o: Partial<EewEvent> = {}) => ({ kind: "eew", cancelled: false, test: false, warning: true, max_scale: 50, ...o }) as EewEvent;

test("quake: 3 or more is medium, up to 2 is low, unknown is silent", () => {
  assert.equal(alertLevel(quake(30), true, false), "medium");
  assert.equal(alertLevel(quake(55), true, false), "medium");
  assert.equal(alertLevel(quake(20), true, false), "low");
  assert.equal(alertLevel(quake(10), true, false), "low");
  assert.equal(alertLevel(quake(-1), true, false), null);
});

test("a later eew report sounds only when its scale rises above the earlier ones", () => {
  const f = (max_scale: number) => eew({ warning: false, max_scale });
  assert.equal(alertLevel(f(40), false, false, 20), "medium");
  assert.equal(alertLevel(f(40), false, false, 40), null);
  assert.equal(alertLevel(f(30), false, false, 40), null);
  // 震度不明 (-1) からの上がりも鳴らす。不明のままなら鳴らさない
  assert.equal(alertLevel(f(30), false, false, -1), "medium");
  assert.equal(alertLevel(f(-1), false, false, -1), null);
  assert.equal(alertLevel(eew({ test: true, warning: false, max_scale: 40 }), false, false, 20), null);
  assert.equal(alertLevel(eew({ cancelled: true, warning: false, max_scale: 40 }), false, false, 20), null);
});

test("a later quake report plays the info sound whatever its type", () => {
  assert.equal(alertLevel({ ...quake(50), info_type: "destination" }, false, false), "info");
  assert.equal(alertLevel({ ...quake(50), info_type: "scale_prompt" }, false, true), "info");
});

test("every report that changes the screen sounds in the Tokara sequence", () => {
  // 2026-10-02 04:05 トカラ列島近海: EEW 6 報 (震度不明 → 3)、震度速報・震源・各地の震度
  const eews = [-1, 30, 30, 30, 30, 30].map((s) => eew({ warning: false, max_scale: s }));
  const got = eews.map((e, i) => {
    const prevMax = Math.max(-1, ...eews.slice(0, i).map((x) => x.max_scale));
    return alertLevel(e, i === 0, false, prevMax);
  });
  assert.deepEqual(got, [null, "medium", null, null, null, null]);
  // 震度速報は同じ地震の EEW が鳴っているので最初でも案内音、続く 2 報も案内音
  assert.equal(alertLevel({ ...quake(30), info_type: "scale_prompt" }, true, true), "info");
  assert.equal(alertLevel({ ...quake(-1), info_type: "destination" }, false, true), "info");
  assert.equal(alertLevel({ ...quake(30), info_type: "detail_scale" }, false, true), "info");
});

test("eew is strong unless cancelled or a drill; it turns the following first quake report into the info sound", () => {
  assert.equal(alertLevel(eew(), true, false), "strong");
  assert.equal(alertLevel(eew({ cancelled: true }), true, false), null);
  assert.equal(alertLevel(eew({ test: true }), true, false), null);
  assert.equal(alertLevel(eew({ test: true, source: "replay" }), true, false), "strong");
  assert.equal(alertLevel(eew({ test: true, source: "demo" }), true, false), "strong");
  assert.equal(alertLevel(quake(50), true, true), "info");
});

test("the detailed intensity report after the first one plays the info sound", () => {
  assert.equal(alertLevel({ ...quake(50), info_type: "detail_scale" }, false, false), "info");
  assert.equal(alertLevel({ ...quake(50), info_type: "detail_scale" }, false, true), "info");
  // 最初の報がいきなり各地の震度なら震度で鳴らす
  assert.equal(alertLevel({ ...quake(30), info_type: "detail_scale" }, true, false), "medium");
});

test("eew forecast sounds by the predicted intensity like a quake report", () => {
  assert.equal(alertLevel(eew({ warning: false, max_scale: 40 }), true, false), "medium");
  assert.equal(alertLevel(eew({ warning: false, max_scale: 20 }), true, false), "low");
  assert.equal(alertLevel(eew({ warning: false, max_scale: -1 }), true, false), null);
});
