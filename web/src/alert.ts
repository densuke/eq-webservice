// 受信した情報で鳴らす警戒音の強さを決める。
// 初動の情報には震度が入っている (EEW 警報は予測震度5弱以上で出る / 震度速報は観測震度) ので、
// マグニチュードや深さからの推定はせず震度で分ける。
// 画面に変化が出る報ごとに鳴らす:
// - EEW: 最初の報 (予報から警報に上がった報も) と、同じ地震のそれまでの最大震度を超えた報
// - 地震情報: 最初の報は震度で、緊急地震速報で既に鳴らしていれば案内音。続く報はどれも案内音

import type { EqEvent } from "./types.ts";

export type AlertLevel = "strong" | "medium" | "low" | "info";

/**
 * firstOfGroup: 同じ地震の最初の情報か (EEW は予報から警報に上がったときも最初とみなす)。
 * eewActive: 直前に EEW で鳴らしていれば地震情報の最初の報は案内音にする
 * prevMaxScale: 同じ地震のそれまでの EEW の最大震度 (震度不明は -1)。最初でない EEW が震度を上げたかの判断に使う
 */
export function alertLevel(e: EqEvent, firstOfGroup: boolean, eewActive: boolean, prevMaxScale = -1): AlertLevel | null {
  if (e.kind === "eew") {
    // 訓練報は鳴らさない。ただしデモ (サーバの再生データ・画面のデモモード) の訓練報は確認のため鳴らす
    if (e.cancelled || (e.test && e.source !== "replay" && e.source !== "demo")) return null;
    // 最初でない報は、震度が上がったときだけ新しい震度で鳴らす
    if (!firstOfGroup) return e.max_scale > prevMaxScale ? byScale(e.max_scale) : null;
    // 予報は地震情報と同じく予測震度で分ける
    return e.warning ? "strong" : byScale(e.max_scale);
  }
  if (e.kind !== "quake") return null;
  if (!firstOfGroup || eewActive) return "info";
  return byScale(e.max_scale);
}

function byScale(s: number): AlertLevel | null {
  if (s >= 30) return "medium";
  if (s > 0) return "low";
  return null;
}
