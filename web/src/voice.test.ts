import { test } from "node:test";
import assert from "node:assert/strict";
import { enqueue, voiceUrl, MAX_VOICES } from "./voice.ts";
import type { Item } from "./voice.ts";

const it = (id: string, group: string): Item => ({ id, group });

test("empty queue: item is appended", () => {
  assert.deepEqual(enqueue([], it("a", "g1")), [it("a", "g1")]);
});

test("different groups queue in order", () => {
  const q = enqueue([it("a", "g1")], it("b", "g2"));
  assert.deepEqual(q, [it("a", "g1"), it("b", "g2")]);
});

test("same group replaces the waiting item at its position", () => {
  const q = enqueue([it("a", "g1"), it("b", "g2"), it("c", "g3")], it("b2", "g2"));
  assert.deepEqual(q, [it("a", "g1"), it("b2", "g2"), it("c", "g3")]);
});

test("input array is not mutated", () => {
  const input = [it("a", "g1"), it("b", "g2")];
  const copy = structuredClone(input);
  const out = enqueue(input, it("c", "g3"));
  assert.notEqual(out, input);
  assert.deepEqual(input, copy);
  enqueue(input, it("b2", "g2"));
  assert.deepEqual(input, copy);
});

test("overflow drops the oldest, keeping the last MAX_VOICES", () => {
  assert.equal(MAX_VOICES, 4);
  let q: Item[] = [];
  for (const n of [1, 2, 3, 4, 5]) q = enqueue(q, it(`i${n}`, `g${n}`));
  assert.deepEqual(q.map((x) => x.id), ["i2", "i3", "i4", "i5"]);
});

test("voiceUrl encodes the id and resolves relative to base", () => {
  assert.equal(voiceUrl("a/b c", "https://eq.example/app/"), "https://eq.example/app/api/tts/event/a%2Fb%20c");
  assert.equal(voiceUrl("x", "https://eq.example/"), "https://eq.example/api/tts/event/x");
});
