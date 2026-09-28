// 離れた場所で同時に起きた地震を、一定の間隔で順に見せる (巡回)。

import { distanceKm } from "./waves.ts";

/** 震央どうしがこれ以上離れていれば、1 つの画面に収めずに巡回する */
export const TOUR_MIN_KM = 300;

/** 巡回するか: 2 つ以上あり、どれか 2 つが TOUR_MIN_KM 以上離れている */
export function worthTouring(epicenters: { lat: number; lon: number }[]): boolean {
  return epicenters.some((a, i) => epicenters.slice(i + 1).some((b) => distanceKm(a.lat, a.lon, b.lat, b.lon) >= TOUR_MIN_KM));
}

/** 巡回を始めてからの経過で、今見せる地震の順番 */
export function tourIndex(startMs: number, now: number, intervalSec: number, count: number): number {
  return Math.floor((now - startMs) / (intervalSec * 1000)) % count;
}
