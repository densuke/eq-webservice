// 2026-10-25 小笠原諸島西方沖: 震度速報 11:12:31 のあと、震源の情報 11:15:37 まで 3 分空いた
import { test } from "node:test";
import assert from "node:assert/strict";
import { AWAIT_MS, attentionDue, awaitingFollowUp } from "./attention.ts";
import type { QuakeGroup } from "./groups.ts";
import type { QuakeEvent } from "./types.ts";

const T = 1_000_000;
const prompt = { kind: "quake", info_type: "scale_prompt", received_at_ms: T, hypocenter: null } as unknown as QuakeEvent;
const dest = { ...prompt, info_type: "destination", hypocenter: { name: "小笠原諸島西方沖" } } as unknown as QuakeEvent;
const group = (events: QuakeEvent[], updatedAt: number): QuakeGroup => ({ key: "q", kind: "quake", updatedAt, events });

test("a scale prompt without a follow-up waits, then asks for attention once", () => {
  const g = group([prompt], T);
  assert.equal(awaitingFollowUp(g, T + AWAIT_MS - 1), false);
  assert.equal(awaitingFollowUp(g, T + AWAIT_MS), true);
  assert.equal(attentionDue(g, T + AWAIT_MS), true);
  assert.equal(attentionDue(g, T + AWAIT_MS + 30_000), false);
  assert.equal(awaitingFollowUp(g, T + 9 * 60_000), true);
  assert.equal(awaitingFollowUp(g, T + 10 * 60_000), false);
});

test("a hypocenter report ends the wait", () => {
  assert.equal(awaitingFollowUp(group([prompt, dest], T + 60_000), T + 60_000 + AWAIT_MS), false);
});
