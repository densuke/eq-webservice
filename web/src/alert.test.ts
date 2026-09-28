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

test("only the first report of an earthquake sounds", () => {
  assert.equal(alertLevel({ ...quake(50), info_type: "destination" }, false, false), null);
  assert.equal(alertLevel(eew(), false, false), null);
});

test("eew is strong unless cancelled or a drill; it silences the following quake report", () => {
  assert.equal(alertLevel(eew(), true, false), "strong");
  assert.equal(alertLevel(eew({ cancelled: true }), true, false), null);
  assert.equal(alertLevel(eew({ test: true }), true, false), null);
  assert.equal(alertLevel(eew({ test: true, source: "replay" }), true, false), "strong");
  assert.equal(alertLevel(quake(50), true, true), null);
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
