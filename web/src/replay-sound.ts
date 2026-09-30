// 履歴の再生の音: 鳴らすかどうかの判定 (DOM にも音にも触らない純粋な関数)。

import type { AlertLevel } from "./alert.ts";
import type { EqEvent } from "./types.ts";
import { WAVE_MAX_SEC } from "./waves.ts";

/** 波の広がり中の刻み (2 秒ごと) の枠の番号。鳴らさないときは -1。ライブと同じ条件で、早送りの最中は鳴らさない */
export function pipSlot(now: number, o: { waving: boolean; replay: boolean; speed: number; fastForward: boolean }): number {
  return o.waving && !o.replay && o.speed <= 1 && !o.fastForward ? Math.floor(now / 2000) : -1;
}

/** 揺れの始まりの音の進み具合: 前回見た時刻 (当時の時刻) と、もう鳴らしたか */
export interface StartSound {
  last: number;
  done: boolean;
}

export const startSoundInit: StartSound = { last: -Infinity, done: false };

/**
 * 再生の時計が next (当時の時刻) に進んだとき、揺れの始まりの音を鳴らすか。1 回の再生で 1 回だけ。
 * 発生時刻 originMs をまたいだ (または、再生の始まりで既に波が出ている) ときだけ鳴らす。
 * jumped (位置のつまみや早送りで飛んだ) ときは、またいでも鳴らさない
 */
export function stepStartSound(s: StartSound, next: number, originMs: number | null, jumped: boolean): { state: StartSound; play: boolean } {
  const crossed = originMs != null && s.last < originMs && originMs <= next && next - originMs <= WAVE_MAX_SEC * 1000;
  return { state: { last: next, done: s.done || crossed }, play: crossed && !jumped && !s.done };
}

/** 揺れの始まりの音の強さ: 最大震度 (後から分かる分まで見る) が 3 以上ならチャイム、それ未満ならピンポン。警報の音 (strong) は緊急地震速報の報に任せる */
export function startSoundLevel(events: EqEvent[]): AlertLevel {
  const max = Math.max(0, ...events.flatMap((e) => (e.kind === "eew" || e.kind === "quake" ? [e.max_scale] : [])));
  return max >= 30 ? "medium" : "low";
}
