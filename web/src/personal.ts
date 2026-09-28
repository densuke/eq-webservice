// 自分の地点と通知の設定 (端末の中だけに保存)、通知するかの判定、主要動の到達までの秒数。

import type { Station } from "./detail.ts";
import type { TsunamiArea } from "./types.ts";
import { arrivalSec, distanceKm, VS_KM_S } from "./waves.ts";

export type NotifyLevel = "off" | "warning" | "4" | "3";

export interface Settings {
  home: { lat: number; lon: number } | null;
  notify: NotifyLevel;
}

const KEY = "eq-settings";
const DEFAULTS: Settings = { home: null, notify: "4" };

export function loadSettings(): Settings {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    const home = v.home && Number.isFinite(v.home.lat) && Number.isFinite(v.home.lon) ? { lat: v.home.lat, lon: v.home.lon } : null;
    const notify: NotifyLevel = ["off", "warning", "4", "3"].includes(v.notify) ? v.notify : DEFAULTS.notify;
    return { home, notify };
  } catch {
    return { ...DEFAULTS };
  }
}

export function saveSettings(s: Settings): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(s));
  } catch {
    // 保存できなくてもこのページの間は有効
  }
}

/** 地点に最も近い震度観測点が属する細分区域 */
export function nearestArea(p: { lat: number; lon: number }, stations: Map<string, Station>): string | null {
  let best: string | null = null;
  let bestD = Infinity;
  for (const s of stations.values()) {
    const d = (s.lat - p.lat) ** 2 + ((s.lon - p.lon) * Math.cos((p.lat * Math.PI) / 180)) ** 2;
    if (d < bestD) [best, bestD] = [s.area, d];
  }
  return best;
}

/** 通知の判定に使う震度 (津波予報は等級を震度に読み替える) */
export const notifyScale = {
  tsunami(grade: TsunamiArea["grade"]): number {
    return { unknown: 0, watch: 30, warning: 45, major_warning: 70 }[grade];
  },
};

/**
 * 通知するか。warning は緊急地震速報の警報 (または津波警報以上)、maxScale は最大震度 (予測・観測)、
 * homeScale は自分の地点の震度 (分からなければ null)
 */
export function shouldNotify(level: NotifyLevel, x: { warning: boolean; maxScale: number; homeScale: number | null }): boolean {
  switch (level) {
    case "off":
      return false;
    case "warning":
      return x.warning;
    case "4":
      return x.warning || x.maxScale >= 40 || (x.homeScale ?? 0) >= 30;
    case "3":
      return x.warning || x.maxScale >= 30;
  }
}

/** 自分の地点に S波 (主要動) が届くまでの秒数 (速度一定の概算) */
export function countdown(
  home: { lat: number; lon: number },
  epi: { lat: number; lon: number; depth: number },
  originMs: number,
  now: number,
): { remainingSec: number; arrived: boolean; distKm: number } {
  const distKm = distanceKm(home.lat, home.lon, epi.lat, epi.lon);
  const remainingSec = (originMs + arrivalSec(VS_KM_S, epi.depth, distKm) * 1000 - now) / 1000;
  return { remainingSec, arrived: remainingSec <= 0, distKm };
}
