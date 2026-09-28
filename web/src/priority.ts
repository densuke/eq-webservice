// 複数の地震が重なったときの優先度と、同じ地震かどうかの判定。

import type { Scale } from "./types.ts";
import { distanceKm } from "./waves.ts";

/** EEW と地震情報を同じ地震とみなす発生時刻の差 (地震情報の発生時刻は分単位) */
export const SAME_QUAKE_MS = 90_000;
/** 震央がこれ以上離れていれば別の地震 */
export const SAME_QUAKE_KM = 200;

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
