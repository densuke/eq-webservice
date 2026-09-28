// 受信した情報で鳴らす警戒音の強さを決める。
// 初動の情報には震度が入っている (EEW 警報は予測震度5弱以上で出る / 震度速報は観測震度) ので、
// マグニチュードや深さからの推定はせず震度で分ける。
// 最初の報のあとに「各地の震度に関する情報」(確定) が届いたら案内音を鳴らす。

import type { EqEvent } from "./types.ts";

export type AlertLevel = "strong" | "medium" | "low" | "info";

/**
 * firstOfGroup: 同じ地震の最初の情報か (EEW は予報から警報に上がったときも最初とみなす)。
 * eewActive: 直前に EEW で鳴らしていれば地震情報では鳴らさない
 */
export function alertLevel(e: EqEvent, firstOfGroup: boolean, eewActive: boolean): AlertLevel | null {
  if (e.kind === "eew") {
    // 訓練報は鳴らさない。ただしデモ (再生データ) の訓練報は確認のため鳴らす
    if (!firstOfGroup || e.cancelled || (e.test && e.source !== "replay")) return null;
    // 予報は地震情報と同じく予測震度で分ける
    return e.warning ? "strong" : byScale(e.max_scale);
  }
  if (e.kind !== "quake") return null;
  if (!firstOfGroup) return e.info_type === "detail_scale" ? "info" : null;
  if (eewActive) return null;
  return byScale(e.max_scale);
}

function byScale(s: number): AlertLevel | null {
  if (s >= 30) return "medium";
  if (s > 0) return "low";
  return null;
}
