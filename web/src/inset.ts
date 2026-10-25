// 離島の別枠に置く震央の印の位置。native の frame.rs の `marker` と同じ規則。

import type { Box } from "./camera.ts";

/** 別枠の範囲の外でも、この度数以内の震央は枠の縁に寄せて印を置く / そのときの枠の縁からの余白 (px) */
export const INSET_MARGIN_DEG = 1.5;
export const INSET_PAD_PX = 8;

/** 範囲を (mx, my) 広げた中の点を、枠の縁から pad 内側に寄せて返す。その外なら null (単位は地図座標) */
export function insetMarkerPos(x: number, y: number, b: Box, mx: number, my: number, pad: number): [number, number] | null {
  if (x < b.x0 - mx || x > b.x1 + mx || y < b.y0 - my || y > b.y1 + my) return null;
  const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(v, lo + pad), hi - pad);
  return [clamp(x, b.x0, b.x1), clamp(y, b.y0, b.y1)];
}

/**
 * 外接矩形の和。離島の別枠の範囲 (islands) に中心があるものは、本土側に 1 つでもあれば外す (別枠に任せる)。
 * 離島だけが揺れたときは、その離島の範囲 (native の shaken::mainland_first と同じ)
 */
export function mainlandFirst(boxes: Box[], islands: Box[]): Box | null {
  const inIsland = (b: Box) => {
    const [cx, cy] = [(b.x0 + b.x1) / 2, (b.y0 + b.y1) / 2];
    return islands.some((i) => cx >= i.x0 && cx <= i.x1 && cy >= i.y0 && cy <= i.y1);
  };
  const main = boxes.filter((b) => !inIsland(b));
  const use = main.length ? main : boxes;
  return use.reduce<Box | null>((a, b) => (a ? { x0: Math.min(a.x0, b.x0), y0: Math.min(a.y0, b.y0), x1: Math.max(a.x1, b.x1), y1: Math.max(a.y1, b.y1) } : b), null);
}
