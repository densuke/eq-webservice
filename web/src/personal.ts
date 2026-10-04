// 自分の地点と通知の設定 (端末の中だけに保存)、通知するかの判定、主要動の到達までの秒数。

import type { Station } from "./detail.ts";
import type { TsunamiArea } from "./types.ts";
import { arrivalSec, distanceKm, VS_KM_S } from "./waves.ts";

export type NotifyLevel = "off" | "warning" | "4" | "3";

interface Settings {
  home: { lat: number; lon: number } | null;
  notify: NotifyLevel;
  /** 離れた地震を巡回する間隔 (秒)。0 は巡回しない */
  tourSec: number;
  /** 観測点の一覧を最後の発表から何分で畳むか。0 は最初から畳む、-1 は自動で畳まない */
  collapseMin: number;
  /** 平時の BGM を流すか */
  bgm: boolean;
  /** BGM の音量 (0〜100) */
  bgmVolume: number;
  /** 警報などを音声で読み上げるか */
  voice: boolean;
  /** 履歴に出す地震情報の最大震度の下限 (0 はすべて) */
  listMin: number;
  /** 平時の天気の札を「今」と「明日」で切り替える間隔 (秒)。0 は今だけ、WEATHER_OFF は札を出さない */
  weatherFlipSec: number;
}

const KEY = "eq-settings";
export const DEFAULTS: Settings = { home: null, notify: "4", tourSec: 10, collapseMin: 10, bgm: false, bgmVolume: 40, voice: true, listMin: 0, weatherFlipSec: 20 };
/** 履歴のしぼり込み: すべて / 震度2以上 / 3以上 / 4以上 / 5弱以上 */
const LIST_MIN_CHOICES = [0, 20, 30, 40, 45];
const BGM_VOLUMES = [0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
export const TOUR_CHOICES = [0, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60];
export const WEATHER_OFF = -1;
export const WEATHER_FLIP_CHOICES = [WEATHER_OFF, 0, 10, 20, 30, 60];
const COLLAPSE_CHOICES = [0, 5, 10, 30, 60, -1];

/** 保存された値を検査して設定にする (壊れた値・古い版の値は既定に戻す) */
export function normalizeSettings(v: unknown): Settings {
  const o = (v && typeof v === "object" ? v : {}) as Record<string, unknown>;
  const h = o.home as { lat?: unknown; lon?: unknown } | null | undefined;
  const home =
    h && typeof h.lat === "number" && typeof h.lon === "number" && Number.isFinite(h.lat) && Number.isFinite(h.lon) ? { lat: h.lat, lon: h.lon } : null;
  const pick = <T>(x: unknown, choices: readonly T[], d: T): T => (choices.includes(x as T) ? (x as T) : d);
  return {
    home,
    notify: pick(o.notify, ["off", "warning", "4", "3"] as const, DEFAULTS.notify),
    tourSec: pick(o.tourSec, TOUR_CHOICES, DEFAULTS.tourSec),
    collapseMin: pick(o.collapseMin, COLLAPSE_CHOICES, DEFAULTS.collapseMin),
    bgm: o.bgm === true,
    bgmVolume: pick(o.bgmVolume, BGM_VOLUMES, DEFAULTS.bgmVolume),
    // 読み上げは既定で ON にした。以前の保存の voice (false を保存していた人が多い) は読まず、新しい voiceV2 だけを見る
    voice: typeof o.voiceV2 === "boolean" ? o.voiceV2 : DEFAULTS.voice,
    listMin: pick(o.listMin, LIST_MIN_CHOICES, DEFAULTS.listMin),
    weatherFlipSec: pick(o.weatherFlipSec, WEATHER_FLIP_CHOICES, DEFAULTS.weatherFlipSec),
  };
}

export function loadSettings(): Settings {
  try {
    return normalizeSettings(JSON.parse(localStorage.getItem(KEY) ?? "{}"));
  } catch {
    return { ...DEFAULTS };
  }
}

/** 観測点の一覧を開いておくか。userOpen は利用者が自分で開閉したとき (優先する) */
export function listOpen(collapseMin: number, lastIssuedMs: number, now: number, userOpen: boolean | undefined): boolean {
  if (userOpen != null) return userOpen;
  if (collapseMin < 0) return true;
  return now - lastIssuedMs < collapseMin * 60_000;
}

/** 保存する JSON。voice は voiceV2 の名前で書く (古い voice は書かない) */
export function settingsJson(s: Settings): string {
  const { voice, ...rest } = s;
  return JSON.stringify({ ...rest, voiceV2: voice });
}

export function saveSettings(s: Settings): void {
  try {
    localStorage.setItem(KEY, settingsJson(s));
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

/**
 * 震源のマグニチュード・深さ・震央距離から、地点の計測震度を推定する (概算)。
 * 最大速度の距離減衰式 (司・翠川 1999) と、最大速度から計測震度への換算 (翠川ほか 1999) による。
 * 断層の広がりや地盤の違いは考えず、震源距離と標準的な地盤増幅 (1.4 倍) を使う
 */
export function estimateIntensity(mag: number, depthKm: number, distKm: number): number {
  const x = Math.max(Math.hypot(distKm, Math.max(depthKm, 0)), 3);
  const logPgv600 = 0.58 * mag + 0.0038 * depthKm - 1.29 - Math.log10(x + 0.0028 * 10 ** (0.5 * mag)) - 0.002 * x;
  const pgv = 10 ** logPgv600 * 1.4;
  return 2.68 + 1.72 * Math.log10(pgv);
}

/** 震度3 の下限 (計測震度) */
const SHINDO3 = 2.5;

/**
 * 自分の地点のカウントダウンを出すか。homeScale は緊急地震速報で自分の地点の区域に出ている予測震度
 * (区域に含まれていなければ null)、estimated は推定した計測震度 (推定できなければ null)
 */
export function countdownWorthShowing(homeScale: number | null, estimated: number | null): boolean {
  if (homeScale != null) return true;
  return estimated != null && estimated >= SHINDO3;
}

/** 計測震度から震度階級 (P2P地震情報の数値表現) */
export function intensityToScale(i: number): number {
  if (i < 0.5) return 0;
  if (i < 1.5) return 10;
  if (i < 2.5) return 20;
  if (i < 3.5) return 30;
  if (i < 4.5) return 40;
  if (i < 5.0) return 45;
  if (i < 5.5) return 50;
  if (i < 6.0) return 55;
  if (i < 6.5) return 60;
  return 70;
}

/**
 * 履歴に出すか。地震情報は最大震度が min 以上のときだけ (最大震度が分からないものは「すべて」のときだけ)。
 * 緊急地震速報・津波予報などは常に出す
 */
export function shownInList(kind: string, maxScale: number, min: number): boolean {
  if (kind !== "quake" || min <= 0) return true;
  return maxScale >= min;
}
