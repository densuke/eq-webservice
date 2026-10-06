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
  /** 濃さ 0〜1 (1 = 保持時間の内) */
  alpha: number;
}

/** 濃さの刻み。署名の変化 (= 再描画) を 0.05 ごとに抑える */
const ALPHA_STEP = 20;

/** サブの地図に何を出すか。地震 (quake・eew) でなければ null。保持時間 + fadeSec を過ぎたら (濃さが 0 になったら) null = 消える。
 *  保持時間を過ぎたら fadedAlpha から 0 へ直線で薄くなり、0.05 刻みに切り下げる (1e-9 は 0.2 などが浮動小数の誤差で 1 段下がらないため) */
export function subMapState(now: number, g: { key: string; kind: string; updatedAt: number } | undefined, scale: number, cfg: SubMapConfig): SubMapState | null {
  if (!g || (g.kind !== "quake" && g.kind !== "eew")) return null;
  const past = now - g.updatedAt - holdMs(scale, cfg);
  if (past <= 0) return { key: g.key, alpha: 1 };
  const x = cfg.fadedAlpha * (1 - past / (cfg.fadeSec * 1000));
  const alpha = Math.floor(x * ALPHA_STEP + 1e-9) / ALPHA_STEP;
  return alpha > 0 ? { key: g.key, alpha } : null;
}

/** サブの地図の署名を 2 つに分ける。paint は描き直し (塗り・震央・カメラ)、fade は濃さだけ。薄くなるだけで塗りを描き直さないため */
export function subMapSigs(state: SubMapState | null, g: { updatedAt: number } | undefined, w: number, h: number): { paint: string; fade: string } {
  return {
    paint: `${state && g ? `${state.key}|${g.updatedAt}` : ""}|${w}x${h}`,
    fade: state ? String(state.alpha) : "",
  };
}

/** 左の地図のカメラの目標。サブの地図が見えている間は日本全体のまま (null)、履歴で選んだときだけ寄る */
export function mainMapTarget<T>(box: T | null, subShown: boolean, selected: boolean): T | null {
  return subShown && !selected ? null : box;
}
