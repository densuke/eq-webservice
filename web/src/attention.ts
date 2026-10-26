// 震度速報のあと震源の情報が来ないまま時間が経ったときの「今後の情報に注意」 (docs/tts.md S15。配信は native/model.rs と同じ規則)
import type { QuakeGroup } from "./groups.ts";

/** 震度速報のあと、震源の情報が来ないままこれだけ経ったら出す */
export const AWAIT_MS = 90_000;
/** 画面に出し続ける上限 (最後の報から) */
const AWAIT_SHOW_MS = 10 * 60_000;
/** 読み上げは AWAIT_MS からこの間だけ (開き直した直後に古い震度速報で読まないように) */
const AWAIT_VOICE_MS = 30_000;

export const ATTENTION_TEXT = "調査中。今後の情報に注意してください";

/** 震源の情報が来ないまま AWAIT_MS 以上経った地震か */
export function awaitingFollowUp(g: QuakeGroup, now: number): boolean {
  const age = now - g.updatedAt;
  return !g.events.some((e) => e.hypocenter) && age >= AWAIT_MS && age < AWAIT_SHOW_MS;
}

/** 読み上げを始める地震か (AWAIT_MS を過ぎた直後だけ。1 回だけ読むのは呼ぶ側が覚える) */
export function attentionDue(g: QuakeGroup, now: number): boolean {
  return awaitingFollowUp(g, now) && now - g.updatedAt < AWAIT_MS + AWAIT_VOICE_MS;
}
