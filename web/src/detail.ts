// 寄ったときの細かい表示: 震度観測点の点と、細分区域ごとの最大震度。

import type { EewArea, ObservationPoint, Scale } from "./types.ts";

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

export interface QuakeDetail {
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

/** 緊急地震速報の区域。予測の上限 (「〜程度以上」なら下限) で塗る */
export function eewAreaScales(areas: EewArea[]): AreaScale[] {
  return maxByArea(areas.map((a) => ({ name: a.name, scale: a.scale_to ?? a.scale_from })));
}
