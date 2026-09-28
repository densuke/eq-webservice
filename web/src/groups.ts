// 同じ地震に関する複数の情報 (震度速報 → 震源情報 → 各地の震度 / EEW の続報) を 1 つにまとめる。

import type { EewEvent, EqEvent, Hypocenter, ObservationPoint, PrefScale, QuakeEvent, Scale } from "./types.ts";

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
export interface SingleGroup {
  key: string;
  kind: "tsunami" | "eew_detection";
  updatedAt: number;
  events: EqEvent[];
}
export type Group = QuakeGroup | EewGroup | SingleGroup;

export function groupKey(e: EqEvent): string {
  switch (e.kind) {
    case "quake":
      // 発生時刻 (分単位) が同じものは同じ地震とみなす
      return `q:${e.origin_time.slice(0, 16)}`;
    case "eew":
      return `e:${e.event_id}`;
    default:
      return `${e.kind}:${e.id}`;
  }
}

export class GroupStore {
  private groups = new Map<string, Group>();
  private ids = new Set<string>();

  /** 追加して所属グループを返す。既知の ID なら null */
  add(e: EqEvent): Group | null {
    if (this.ids.has(e.id)) return null;
    this.ids.add(e.id);
    const key = groupKey(e);
    let g = this.groups.get(key);
    if (!g) {
      g = { key, kind: e.kind === "quake" || e.kind === "eew" ? e.kind : e.kind, updatedAt: 0, events: [] } as Group;
      this.groups.set(key, g);
    }
    (g.events as EqEvent[]).push(e);
    g.updatedAt = Math.max(g.updatedAt, e.received_at_ms);
    this.prune();
    return g;
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

export interface QuakeSummary {
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
