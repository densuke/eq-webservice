import { test } from "node:test";
import assert from "node:assert/strict";
import { isDisabled, viewersLabel } from "./viewers.ts";

test("a server that never fetched (updated_ms 0) means disabled", () => {
  assert.equal(isDisabled({ viewers: null, video_id: "", updated_ms: 0 }), true);
  assert.equal(isDisabled({ viewers: null, updated_ms: 1 }), false);
  assert.equal(isDisabled(null), false);
});

const now = 1_000_000_000;

test("shows the count with the unit; thousands are grouped", () => {
  assert.equal(viewersLabel({ viewers: 12, updated_ms: now - 1000 }, now), "同接 12 人");
  assert.equal(viewersLabel({ viewers: 1234, updated_ms: now }, now), "同接 1,234 人");
  assert.equal(viewersLabel({ viewers: 0, updated_ms: now }, now), "同接 0 人");
});

test("hides when null, stale, or the shape is wrong", () => {
  assert.equal(viewersLabel({ viewers: null, updated_ms: now }, now), null);
  assert.equal(viewersLabel({ viewers: 5, updated_ms: now - 6 * 60_000 }, now), null);
  assert.equal(viewersLabel({ viewers: 5 }, now), null);
  assert.equal(viewersLabel({ viewers: "5", updated_ms: now }, now), null);
  assert.equal(viewersLabel({ viewers: -1, updated_ms: now }, now), null);
  assert.equal(viewersLabel(null, now), null);
  assert.equal(viewersLabel("x", now), null);
});
