import { test } from "node:test";
import assert from "node:assert/strict";
import { GroupStore } from "./groups.ts";
import type { EewDetectionEvent } from "./types.ts";

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
