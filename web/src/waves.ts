// P波・S波の到達範囲の計算。
//
// いまは一様な速度構造 (Vp, Vs 一定) による近似。震源距離 R = v·t から、
// 地表での到達半径 r = √(R² − 深さ²) を求める。
// 気象庁の走時表 (JMA2001) を使うとより正確になる (将来の改善点)。

/** P波・S波を描く最短の秒数。近い地震は今までどおりこれで止める */
export const WAVE_MAX_SEC = 180;
/** P波・S波を描く最長の秒数 */
export const WAVE_LIMIT_SEC = 480;

export const VP_KM_S = 6.5;
export const VS_KM_S = 3.75;

/** 円の広がりを maxKm で止める (揺れの範囲を越えて日本全体を覆わないように)。未到達 (null) はそのまま */
export function capKm(r: number | null, maxKm: number): number | null {
  return r == null ? null : Math.min(r, maxKm);
}

/** 震源の深さ depthKm、発生からの経過秒 elapsedSec での地表到達半径 (km)。未到達なら null */
export function surfaceRadiusKm(velocity: number, depthKm: number, elapsedSec: number): number | null {
  if (elapsedSec <= 0) return null;
  const r = velocity * elapsedSec;
  const d = Math.max(depthKm, 0);
  if (r <= d) return null;
  return Math.sqrt(r * r - d * d);
}

/** 震央距離 distKm の地点に波が届くまでの秒数 */
export function arrivalSec(velocity: number, depthKm: number, distKm: number): number {
  return Math.hypot(distKm, Math.max(depthKm, 0)) / velocity;
}

/**
 * P波・S波を描く秒数。S 波が地表で reachKm (揺れた範囲の一番遠い所) に届くまで。短くても WAVE_MAX_SEC、長くても WAVE_LIMIT_SEC。
 * 深い・遠い地震は S 波が遅れて地表に出るので、3 分では揺れた範囲の端まで届かない (native の eew::wave_life_ms と同じ)
 */
export function waveLifeSec(depthKm: number, reachKm: number): number {
  return Math.min(Math.max(arrivalSec(VS_KM_S, depthKm, reachKm) + 5, WAVE_MAX_SEC), WAVE_LIMIT_SEC);
}

const EARTH_RADIUS_KM = 6371;
const toRad = (d: number) => (d * Math.PI) / 180;
const toDeg = (r: number) => (r * 180) / Math.PI;

/** 大円距離 (km) */
export function distanceKm(lat1: number, lon1: number, lat2: number, lon2: number): number {
  const dLat = toRad(lat2 - lat1);
  const dLon = toRad(lon2 - lon1);
  const a = Math.sin(dLat / 2) ** 2 + Math.cos(toRad(lat1)) * Math.cos(toRad(lat2)) * Math.sin(dLon / 2) ** 2;
  return 2 * EARTH_RADIUS_KM * Math.asin(Math.min(1, Math.sqrt(a)));
}

/** 中心から半径 radiusKm の円周 (測地線) を n 点の [lon, lat] で返す */
export function geoCircle(lat: number, lon: number, radiusKm: number, n = 90): [number, number][] {
  const out: [number, number][] = [];
  const φ1 = toRad(lat);
  const λ1 = toRad(lon);
  const δ = radiusKm / EARTH_RADIUS_KM;
  for (let i = 0; i <= n; i++) {
    const θ = (2 * Math.PI * i) / n;
    const φ2 = Math.asin(Math.sin(φ1) * Math.cos(δ) + Math.cos(φ1) * Math.sin(δ) * Math.cos(θ));
    const λ2 = λ1 + Math.atan2(Math.sin(θ) * Math.sin(δ) * Math.cos(φ1), Math.cos(δ) - Math.sin(φ1) * Math.sin(φ2));
    out.push([toDeg(λ2), toDeg(φ2)]);
  }
  return out;
}
