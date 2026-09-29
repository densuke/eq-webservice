// 地震感知情報 (P2P地震情報の利用者による「揺れた」報告の集計)。気象庁の発表ではない。

import { parseJst } from "./demo.ts";
import type { UserquakeEvent } from "./types.ts";

/** 最後の更新からこの時間は地図に出す */
const USERQUAKE_SHOW_MS = 2 * 60_000;
/** 報告の始まりのこの時間前以降に気象庁の地震の情報が届いていれば、その揺れの報告とみなして出さない */
const OFFICIAL_LEAD_MS = 30_000;

/** 同じ揺れの評価は新しい方を残す。別の揺れなら新しく始まった方 */
export function latestUserquake(prev: UserquakeEvent | null, e: UserquakeEvent): UserquakeEvent {
  if (!prev) return e;
  return e.updated_at >= prev.updated_at ? e : prev;
}

/**
 * 地図に出すか: 最後の更新から USERQUAKE_SHOW_MS 以内で、その後に気象庁の地震の情報 (緊急地震速報・地震情報) が届いていないとき。
 * officialAt は届いた気象庁の地震の情報の受信時刻
 */
export function userquakeShown(u: UserquakeEvent | null, now: number, officialAt: number[]): boolean {
  if (!u) return false;
  const started = parseJst(u.started_at);
  const updated = parseJst(u.updated_at);
  if (started == null || updated == null || now - updated > USERQUAKE_SHOW_MS) return false;
  return !officialAt.some((t) => t >= started - OFFICIAL_LEAD_MS);
}

/** 地域ごとの信頼度の表示 (P2P地震情報 Beta3 の区分) */
export function confidenceGrade(c: number): "A" | "B" | "C" | "D" | "E" | "F" {
  if (c < 0) return "F";
  if (c < 0.2) return "E";
  if (c < 0.4) return "D";
  if (c < 0.6) return "C";
  if (c < 0.8) return "B";
  return "A";
}
