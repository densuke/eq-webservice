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

/** サブの地図の署名を 2 つに分ける。paint は描き直し (塗り・震央・カメラ)、fade は薄さだけ。薄くなるだけで塗りを描き直さないため */
export function subMapSigs(state: SubMapState | null, g: { updatedAt: number } | undefined, w: number, h: number, fadedAlpha: number): { paint: string; fade: string } {
  return {
    paint: `${state && g ? `${state.key}|${g.updatedAt}` : ""}|${w}x${h}`,
    fade: state ? `${state.faded}|${fadedAlpha}` : "",
  };
}

/** 左の地図のカメラの目標。サブの地図が見えている間は日本全体のまま (null)、履歴で選んだときだけ寄る */
export function mainMapTarget<T>(box: T | null, subShown: boolean, selected: boolean): T | null {
  return subShown && !selected ? null : box;
}
