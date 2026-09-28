// 受信した情報で鳴らす警戒音の強さを決める。
// 初動の情報には震度が入っている (EEW 警報は予測震度5弱以上で出る / 震度速報は観測震度) ので、
// マグニチュードや深さからの推定はせず震度で分ける。
// 最初の報のあとに「各地の震度に関する情報」(確定) が届いたら案内音を鳴らす。

import type { EqEvent } from "./types.ts";

export type AlertLevel = "strong" | "medium" | "low" | "info";

/** firstOfGroup: 同じ地震の最初の情報か。eewActive: 直前に EEW で鳴らしていれば地震情報では鳴らさない */
export function alertLevel(e: EqEvent, firstOfGroup: boolean, eewActive: boolean): AlertLevel | null {
  if (e.kind === "eew") {
    // 訓練報は鳴らさない。ただしデモ (再生データ) の訓練報は確認のため鳴らす
    return !firstOfGroup || e.cancelled || (e.test && e.source !== "replay") ? null : "strong";
  }
  if (e.kind !== "quake") return null;
  if (!firstOfGroup) return e.info_type === "detail_scale" ? "info" : null;
  if (eewActive) return null;
  if (e.max_scale >= 30) return "medium";
  if (e.max_scale > 0) return "low";
  return null;
}
