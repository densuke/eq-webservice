// 同じ地震に関する複数の情報 (震度速報 → 震源情報 → 各地の震度 / EEW の続報) を 1 つにまとめる。

import type { EewArea, EewEvent, EqEvent, Hypocenter, ObservationPoint, PrefScale, QuakeEvent, Scale } from "./types.ts";

export interface QuakeGroup {
  key: string;
  kind: "quake";
  updatedAt: number;
  events: QuakeEvent[];
}
export interface EewGroup {
  key: string;
  kind: "eew";
  updatedAt: number;
  events: EewEvent[];
}
interface SingleGroup {
  key: string;
  kind: "tsunami" | "eew_detection";
  updatedAt: number;
  events: EqEvent[];
}
export type Group = QuakeGroup | EewGroup | SingleGroup;

function groupKey(e: EqEvent): string {
  return e.kind === "eew" ? `e:${e.event_id}` : `${e.kind}:${e.id}`;
}

/** グループ内で分かっている震源名 (最初の震度速報には無い) */
function quakeName(g: QuakeGroup): string {
  return g.events.find((e) => e.hypocenter?.name)?.hypocenter?.name ?? "";
}

export class GroupStore {
  private groups = new Map<string, Group>();
  /** 重複排除用に覚えておく ID (古いものから忘れる) */
  private ids = new Set<string>();
  private idOrder: string[] = [];

  private maxIds: number;

  constructor(maxIds = 5000) {
    this.maxIds = maxIds;
  }

  /** 追加して所属グループを返す。既知の ID なら null */
  add(e: EqEvent): Group | null {
    if (this.ids.has(e.id)) return null;
    this.ids.add(e.id);
    this.idOrder.push(e.id);
    if (this.idOrder.length > this.maxIds) {
      this.ids.delete(this.idOrder.shift()!);
    }
    const key = e.kind === "quake" ? this.quakeKey(e) : groupKey(e);
    let g = this.groups.get(key);
    if (!g) {
      // -Infinity: 1970 年より前の記録の場面では受信時刻が負になる (0 だと全グループが 0 に潰れる)
      g = { key, kind: e.kind, updatedAt: -Infinity, events: [] } as Group;
      this.groups.set(key, g);
    }
    (g.events as EqEvent[]).push(e);
    g.updatedAt = Math.max(g.updatedAt, e.received_at_ms);
    this.prune();
    return g;
  }

  /**
   * 地震情報には地震ごとの ID が無いので、発生時刻 (分) と震源名でまとめる。
   * 最初の震度速報には震源が無いため、後から来た震源付きの情報は、同じ分の震源の無いグループに
   * 揺れた都道府県が重なれば合流させる (重ならなければ同じ分に起きた別の地震とみなす)。
   */
  private quakeKey(e: QuakeEvent): string {
    const minute = `q:${e.origin_time.slice(0, 16)}`;
    const inMinute = [...this.groups.values()].filter((g): g is QuakeGroup => g.kind === "quake" && g.key.startsWith(minute));
    const orphans = inMinute.filter((g) => !quakeName(g));
    const name = e.hypocenter?.name ?? "";
    if (!name) return orphans[0]?.key ?? (inMinute.length ? `${minute}:${inMinute.length}` : minute);
    const same = inMinute.find((g) => quakeName(g) === name);
    if (same) return same.key;
    const prefs = new Set(e.pref_max.map((p) => p.pref));
    const orphan = orphans.find((g) => prefs.size === 0 || g.events.some((x) => x.pref_max.some((p) => prefs.has(p.pref))));
    return orphan?.key ?? `${minute}:${name}`;
  }

  get(key: string): Group | undefined {
    return this.groups.get(key);
  }

  /** 新しい順 */
  list(): Group[] {
    return [...this.groups.values()].sort((a, b) => b.updatedAt - a.updatedAt);
  }

  private prune(max = 300): void {
    if (this.groups.size <= max) return;
    for (const g of this.list().slice(max)) this.groups.delete(g.key);
  }
}

interface QuakeSummary {
  latest: QuakeEvent;
  infoLabel: string;
  originTime: string;
  originTimeMs: number | null;
  hypocenter: Hypocenter | null;
  maxScale: Scale;
  domesticTsunami: string;
  points: ObservationPoint[];
  prefMax: PrefScale[];
  comment: string;
}

const INFO_LABELS: Record<QuakeEvent["info_type"], string> = {
  scale_prompt: "震度速報",
  destination: "震源に関する情報",
  scale_and_destination: "震源・震度に関する情報",
  detail_scale: "各地の震度に関する情報",
  foreign: "遠地地震に関する情報",
  other: "地震情報",
};

/** グループ内の情報を合成する (後から来た情報を優先しつつ、欠けている項目は前の情報で補う) */
export function summarizeQuake(g: QuakeGroup): QuakeSummary {
  const evs = g.events;
  const latest = evs[evs.length - 1];
  const lastWith = <T>(f: (e: QuakeEvent) => T | null | undefined): T | null => {
    for (let i = evs.length - 1; i >= 0; i--) {
      const v = f(evs[i]);
      if (v != null) return v;
    }
    return null;
  };
  const withPoints = lastWith((e) => (e.points.length > 0 ? e : null));
  return {
    latest,
    infoLabel: INFO_LABELS[latest.info_type],
    originTime: latest.origin_time,
    originTimeMs: lastWith((e) => e.origin_time_ms),
    hypocenter: lastWith((e) => e.hypocenter),
    maxScale: Math.max(...evs.map((e) => e.max_scale)),
    domesticTsunami: latest.domestic_tsunami,
    points: withPoints?.points ?? [],
    prefMax: withPoints?.pref_max ?? [],
    comment: latest.comment,
  };
}

/** EEW は最新の報 (serial が最大のもの) */
export function latestEew(g: EewGroup): EewEvent {
  return g.events.reduce((a, b) => (Number(b.serial) >= Number(a.serial) ? b : a));
}

/** 同じ地震の予想を、報が変わっても最大で持ち続けた EEW (表示用。通知や音など、報ごとに反応するものは latestEew の方) */
export function heldEew(g: EewGroup): EewEvent {
  return heldForecast(g.events);
}

const maxOf = (xs: number[]): number => xs.reduce((a, b) => Math.max(a, b));

/**
 * 同じ地震 (同じ event_id) の報から、予想を最大で持ち続けた 1 件を作る。
 * 地域・県・最大震度は全報の最大、警報は一度でも出ていれば警報のまま。震源などは最新の報。
 * 最新の報が取り消しなら、その報のまま (取り消しは消す)
 */
export function heldForecast(events: EewEvent[]): EewEvent {
  const ordered = [...events].sort((a, b) => Number(a.serial) - Number(b.serial));
  const latest = ordered[ordered.length - 1];
  if (latest.cancelled) return latest;
  const live = ordered.filter((e) => !e.cancelled);
  const areas = new Map<string, EewArea[]>();
  const prefs = new Map<string, number>();
  for (const e of live) {
    for (const a of e.areas) areas.set(a.name, [...(areas.get(a.name) ?? []), a]);
    for (const p of e.pref_max) prefs.set(p.pref, Math.max(prefs.get(p.pref) ?? p.scale, p.scale));
  }
  return {
    ...latest,
    warning: live.some((e) => e.warning),
    max_scale: maxOf(live.map((e) => e.max_scale)),
    pref_max: [...prefs].map(([pref, scale]) => ({ pref, scale })),
    areas: [...areas.values()].map((xs) => ({
      ...xs[xs.length - 1],
      scale_from: maxOf(xs.map((a) => a.scale_from)),
      scale_to: xs.every((a) => a.scale_to == null) ? null : maxOf(xs.map((a) => a.scale_to ?? a.scale_from)),
    })),
  };
}
