// 寄ったときの細かい表示: 震度観測点の点と、細分区域ごとの最大震度。

import { scaleLabel } from "./scale.ts";
import type { EewArea, EewEvent, ObservationPoint, Scale } from "./types.ts";

export interface Station {
  lat: number;
  lon: number;
  /** 属する地震情報細分区域 */
  area: string;
}

export interface AreaScale {
  name: string;
  scale: Scale;
}

interface QuakeDetail {
  dots: { name: string; lat: number; lon: number; scale: Scale }[];
  areas: AreaScale[];
  /** 位置が分からない観測点 (一覧に無い新しい観測点など) */
  missing: string[];
}

/** 区域ごとの最大震度 (出てきた順) */
function maxByArea(items: AreaScale[]): AreaScale[] {
  const m = new Map<string, Scale>();
  for (const { name, scale } of items) m.set(name, Math.max(scale, m.get(name) ?? -1));
  return [...m].map(([name, scale]) => ({ name, scale }));
}

/** 地震情報の観測点。震度速報は区域 (is_area)、各地の震度は観測点で届く */
export function quakeDetail(points: ObservationPoint[], stations: Map<string, Station>): QuakeDetail {
  const dots: QuakeDetail["dots"] = [];
  const areas: AreaScale[] = [];
  const missing: string[] = [];
  for (const p of points) {
    if (p.is_area) {
      areas.push({ name: p.addr, scale: p.scale });
      continue;
    }
    const s = p.station ?? stations.get(p.addr);
    if (!s) {
      missing.push(p.addr);
      continue;
    }
    dots.push({ name: p.addr, lat: s.lat, lon: s.lon, scale: p.scale });
    areas.push({ name: s.area, scale: p.scale });
  }
  return { dots, areas: maxByArea(areas), missing };
}

/** 観測を予測の上に重ねる: 観測のある地域は観測の震度、まだ無い地域は予測のまま (forecast: true) */
export function overlayForecast(observed: AreaScale[], forecast: AreaScale[]): (AreaScale & { forecast: boolean })[] {
  const seen = new Set(observed.map((o) => o.name));
  return [...observed.map((o) => ({ ...o, forecast: false })), ...forecast.filter((f) => !seen.has(f.name)).map((f) => ({ ...f, forecast: true }))];
}

/** 予測を残す上限 (観測の震度がまだ届かないとき。震度の塗りが消えるまでと同じ 1 時間) */
const FORECAST_KEEP_MS = 60 * 60_000;

/**
 * 地震情報を表示しているときに、同じ地震の緊急地震速報の予測を重ねて残すか。
 * 速報が続いている間 (active) は残す。速報が終わっても、観測の震度がまだ届いていなければ (震源の情報だけなど) 上限まで残す
 */
export function keepForecast(active: boolean, observed: boolean, sinceLastReportMs: number): boolean {
  return active || (!observed && sinceLastReportMs <= FORECAST_KEEP_MS);
}

/** 続報で外された予測の地域を、外されてからこの時間は薄れながら残す (いきなり消えると不自然なので) */
const DROPPED_MS = 8000;

/**
 * 続報で外された予測の地域のうち、外されてから ms 以内のもの (最後に予測されたときの震度)。
 * reports は同じ地震の各報 (届いた順)。最後の報にある地域は含めない
 */
export function droppedForecast(reports: { at: number; items: AreaScale[] }[], now: number, ms = DROPPED_MS): AreaScale[] {
  const last = new Map<string, { scale: Scale; i: number }>();
  reports.forEach((r, i) => r.items.forEach(({ name, scale }) => last.set(name, { scale, i })));
  const latest = reports.length - 1;
  return [...last]
    .filter(([, v]) => v.i < latest && now - reports[v.i + 1].at <= ms)
    .map(([name, v]) => ({ name, scale: v.scale }));
}

/** 緊急地震速報の区域。予測の上限 (「〜程度以上」なら下限) で塗る */
export function eewAreaScales(areas: EewArea[]): AreaScale[] {
  return maxByArea(areas.map((a) => ({ name: a.name, scale: a.scale_to ?? a.scale_from })));
}

/**
 * 緊急地震速報の予報の札 (地図の震源の印の近く)。地域ごとの予想も県ごとの予想も空で、塗るものが無いときだけ出す。
 * 取り消し・最大予想震度が分からないときは null
 */
export function forecastTag(e: EewEvent): string | null {
  if (e.cancelled || e.max_scale <= 0 || e.areas.length > 0 || e.pref_max.length > 0) return null;
  return `予測最大震度${scaleLabel(e.max_scale)}`;
}
