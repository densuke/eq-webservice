import { test } from "node:test";
import assert from "node:assert/strict";
import { alertLevel } from "./alert.ts";
import type { EewEvent, QuakeEvent } from "./types.ts";

const quake = (max_scale: number) => ({ kind: "quake", max_scale }) as QuakeEvent;
const eew = (o: Partial<EewEvent> = {}) => ({ kind: "eew", cancelled: false, test: false, ...o }) as EewEvent;

test("quake: 3 or more is medium, up to 2 is low, unknown is silent", () => {
  assert.equal(alertLevel(quake(30), true, false), "medium");
  assert.equal(alertLevel(quake(55), true, false), "medium");
  assert.equal(alertLevel(quake(20), true, false), "low");
  assert.equal(alertLevel(quake(10), true, false), "low");
  assert.equal(alertLevel(quake(-1), true, false), null);
});

test("only the first report of an earthquake sounds", () => {
  assert.equal(alertLevel(quake(50), false, false), null);
  assert.equal(alertLevel(eew(), false, false), null);
});

test("eew is strong unless cancelled or a drill; it silences the following quake report", () => {
  assert.equal(alertLevel(eew(), true, false), "strong");
  assert.equal(alertLevel(eew({ cancelled: true }), true, false), null);
  assert.equal(alertLevel(eew({ test: true }), true, false), null);
  assert.equal(alertLevel(quake(50), true, true), null);
});
