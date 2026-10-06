// サブの地図 (右上の「最近の地震」) を、どの地震をどれだけの濃さで出すかの判断。DOM には触らない。

import type { SubMapConfig } from "./layout.ts";

/** 最後の情報から出し続ける時間 (ms)。scale は最大震度 (10〜70)、0 以下は分からない */
export function holdMs(scale: number, cfg: SubMapConfig): number {
  if (scale <= 0) return cfg.unknownSec * 1000;
  const rule = cfg.hold.find((r) => r.maxScale === undefined || scale <= r.maxScale);
  return (rule?.sec ?? cfg.unknownSec) * 1000;
}

export interface SubMapState {
  key: string;
  /** 保持時間を過ぎた (同じ地震を薄く描く) */
  faded: boolean;
}

/** サブの地図に何を出すか。地震 (quake・eew) でなければ null */
export function subMapState(now: number, g: { key: string; kind: string; updatedAt: number } | undefined, scale: number, cfg: SubMapConfig): SubMapState | null {
  if (!g || (g.kind !== "quake" && g.kind !== "eew")) return null;
  return { key: g.key, faded: now - g.updatedAt > holdMs(scale, cfg) };
}
