import { test } from "node:test";
import assert from "node:assert/strict";
import { type StartSound, pipSlot, startSoundInit, startSoundLevel, stepStartSound } from "./replay-sound.ts";
import type { EqEvent } from "./types.ts";

const ORIGIN = 1_000_000_000_000;
const play = (s: StartSound, next: number, origin: number | null, jumped = false) => stepStartSound(s, next, origin, jumped);

test("揺れの始まり: 発生時刻をまたいだときだけ鳴る", () => {
  const before = play(startSoundInit, ORIGIN - 10_000, ORIGIN);
  assert.equal(before.play, false);
  const cross = play(before.state, ORIGIN + 100, ORIGIN);
  assert.equal(cross.play, true);
  // 1 回の再生で 1 回だけ
  assert.equal(play(cross.state, ORIGIN + 5000, ORIGIN).play, false);
  // ちょうど発生時刻でも鳴る
  assert.equal(play(before.state, ORIGIN, ORIGIN).play, true);
});

test("揺れの始まり: 飛んだときは、またいでも鳴らさず、あとからも鳴らさない", () => {
  const before = play(startSoundInit, ORIGIN - 10_000, ORIGIN);
  const jumped = play(before.state, ORIGIN + 30_000, ORIGIN, true);
  assert.equal(jumped.play, false);
  assert.equal(play(jumped.state, ORIGIN + 35_000, ORIGIN).play, false);
});

test("揺れの始まり: 再生の始まりで既に波が出ているときは、始まりで 1 回鳴る", () => {
  const first = play(startSoundInit, ORIGIN + 93_000, ORIGIN);
  assert.equal(first.play, true);
  assert.equal(play(first.state, ORIGIN + 94_000, ORIGIN).play, false);
});

test("揺れの始まり: 始まりで波が消えているとき・発生時刻が無いときは鳴らさない", () => {
  assert.equal(play(startSoundInit, ORIGIN + 181_000, ORIGIN).play, false);
  assert.equal(play(startSoundInit, ORIGIN, null).play, false);
});

test("刻み: 波が広がっていて、速さ ×1 以下で、早送りでないときだけ", () => {
  const base = { waving: true, replay: false, speed: 1, fastForward: false };
  assert.equal(pipSlot(4001, base), 2);
  assert.equal(pipSlot(4001, { ...base, speed: 0.5 }), 2);
  assert.equal(pipSlot(4001, { ...base, waving: false }), -1);
  assert.equal(pipSlot(4001, { ...base, speed: 2 }), -1);
  assert.equal(pipSlot(4001, { ...base, replay: true }), -1);
  assert.equal(pipSlot(4001, { ...base, fastForward: true }), -1);
});

test("揺れの始まりの強さ: 最大震度 3 以上でチャイム、それ未満でピンポン", () => {
  const q = (max_scale: number) => ({ kind: "quake", max_scale }) as unknown as EqEvent;
  const e = (max_scale: number) => ({ kind: "eew", max_scale }) as unknown as EqEvent;
  assert.equal(startSoundLevel([q(10), q(30)]), "medium");
  assert.equal(startSoundLevel([e(20), q(20)]), "low");
  assert.equal(startSoundLevel([]), "low");
});
