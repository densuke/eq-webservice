// 地図の自動カメラ: 震央へ寄り、S波の広がりに合わせて引き、揺れた地域が収まったら止まる。
// 座標は地図 (map.ts の project) の単位。1 度 ≒ 100 単位なので 1km ≒ 100/111 単位。

export interface Box {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

export const KM_TO_UNITS = 100 / 111;
/** 最初に寄るときの半径 */
export const MIN_RADIUS_KM = 80;
/** 揺れた地域が分からないときに引く上限 */
export const DEFAULT_STOP_KM = 300;

export function pointBox(x: number, y: number, rKm: number): Box {
  const r = rKm * KM_TO_UNITS;
  return { x0: x - r, y0: y - r, x1: x + r, y1: y + r };
}

export function union(a: Box | null, b: Box | null): Box | null {
  if (!a) return b;
  if (!b) return a;
  return { x0: Math.min(a.x0, b.x0), y0: Math.min(a.y0, b.y0), x1: Math.max(a.x1, b.x1), y1: Math.max(a.y1, b.y1) };
}

/** 震央から揺れた地域 (の外接矩形) の一番遠い角までの距離。ここまで引いたら止める */
export function stopRadiusKm(x: number, y: number, shaken: Box | null): number {
  if (!shaken) return DEFAULT_STOP_KM;
  const dx = Math.max(Math.abs(shaken.x0 - x), Math.abs(shaken.x1 - x));
  const dy = Math.max(Math.abs(shaken.y0 - y), Math.abs(shaken.y1 - y));
  return Math.max(MIN_RADIUS_KM, Math.hypot(dx, dy) / KM_TO_UNITS);
}

/** S波の半径 sKm のときに見せる半径 */
export function followRadiusKm(sKm: number | null, stopKm: number): number {
  return Math.min(Math.max(sKm ?? 0, MIN_RADIUS_KM), stopKm);
}

/** 周囲に余白を付け、小さすぎる範囲は minKm 四方まで広げる */
export function pad(b: Box, ratio = 0.15, minKm = 2 * MIN_RADIUS_KM): Box {
  const min = minKm * KM_TO_UNITS;
  const cx = (b.x0 + b.x1) / 2;
  const cy = (b.y0 + b.y1) / 2;
  const hw = Math.max((b.x1 - b.x0) * (1 + ratio), min) / 2;
  const hh = Math.max((b.y1 - b.y0) * (1 + ratio), min) / 2;
  return { x0: cx - hw, y0: cy - hh, x1: cx + hw, y1: cy + hh };
}
