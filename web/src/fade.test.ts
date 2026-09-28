import { test } from "node:test";
import assert from "node:assert/strict";
import { fadeOpacity, FULL_MS, GONE_MS } from "./fade.ts";

test("full for 10 minutes, fades linearly, gone at 60 minutes", () => {
  assert.equal(fadeOpacity(0), 1);
  assert.equal(fadeOpacity(FULL_MS), 1);
  assert.equal(fadeOpacity((FULL_MS + GONE_MS) / 2), 0.5);
  assert.equal(fadeOpacity(GONE_MS), 0);
  assert.equal(fadeOpacity(GONE_MS * 10), 0);
});
