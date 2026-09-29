// 複数の地震が重なったときの優先度と、同じ地震かどうかの判定。

import type { Scale } from "./types.ts";
import { distanceKm } from "./waves.ts";

/** EEW と地震情報を同じ地震とみなす発生時刻の差 (地震情報の発生時刻は分単位) */
export const SAME_QUAKE_MS = 90_000;
/** 震央がこれ以上離れていれば別の地震 */
export const SAME_QUAKE_KM = 200;

/** 最後の情報からこの時間がたち、揺れも描き終えたら、地震の表示をやめて平時 (日本全体・気象警報) に戻す */
export const SETTLE_MS = 3 * 60_000;
/** 軽い地震 (最大震度がこれ以下) は早く戻す */
export const MINOR_SCALE = 20;
export const MINOR_SETTLE_MS = 60_000;

/** その地震の表示を続ける時間 (最大震度が分からないものは通常どおり) */
export function settleMs(scale: Scale): number {
  return scale > 0 && scale <= MINOR_SCALE ? MINOR_SETTLE_MS : SETTLE_MS;
}

export interface Place {
  originMs: number | null;
  lat: number | null;
  lon: number | null;
}

export function sameQuake(a: Place, b: Place): boolean {
  if (a.originMs == null || b.originMs == null) return false;
  if (Math.abs(a.originMs - b.originMs) > SAME_QUAKE_MS) return false;
  if (a.lat == null || a.lon == null || b.lat == null || b.lon == null) return true;
  return distanceKm(a.lat, a.lon, b.lat, b.lon) <= SAME_QUAKE_KM;
}

/** 揺れの大きい方 (EEW は予測、地震情報は観測の最大震度) を先に。同じなら新しい方 */
export function byPriority(a: { scale: Scale; at: number }, b: { scale: Scale; at: number }): number {
  return b.scale - a.scale || b.at - a.at;
}
