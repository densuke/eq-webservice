// 受信した情報で鳴らす警戒音の強さを決める。
// 初動の情報には震度が入っている (EEW 警報は予測震度5弱以上で出る / 震度速報は観測震度) ので、
// マグニチュードや深さからの推定はせず震度で分ける。

import type { EqEvent } from "./types.ts";

export type AlertLevel = "strong" | "medium" | "low";

/** firstOfGroup: 同じ地震の最初の情報か。eewActive: 直前に EEW で鳴らしていれば地震情報では鳴らさない */
export function alertLevel(e: EqEvent, firstOfGroup: boolean, eewActive: boolean): AlertLevel | null {
  if (!firstOfGroup) return null;
  if (e.kind === "eew") return e.cancelled || e.test ? null : "strong";
  if (e.kind !== "quake" || eewActive) return null;
  if (e.max_scale >= 30) return "medium";
  if (e.max_scale > 0) return "low";
  return null;
}
