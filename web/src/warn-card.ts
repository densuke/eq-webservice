// 平時に危険警報以上の区域へ寄せたときに出す札: 区域と重ならない側に置き、区域の中心へ矢印を引く

import { esc } from "./html.ts";
import type { WarnFocus } from "./warnings.ts";

interface Rect {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

/** 札の左上 (x, y) と矢印 (札の縁 from → 区域の中心 to)。すべて画面のピクセル */
export interface CardPlace {
  x: number;
  y: number;
  fromX: number;
  fromY: number;
  toX: number;
  toY: number;
}

const GAP = 48;
const MARGIN = 8;
const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(v, Math.max(lo, hi)));

/** region は区域の外接矩形、card は札の大きさ、screen は地図の枠の大きさ。区域に重ならない候補のうち先のもの (左・下・右・上) */
export function placeCard(region: Rect, card: { w: number; h: number }, screen: { w: number; h: number }): CardPlace {
  const cx = (region.x0 + region.x1) / 2;
  const cy = (region.y0 + region.y1) / 2;
  const fit = ([x, y]: number[]) => [clamp(x, MARGIN, screen.w - card.w - MARGIN), clamp(y, MARGIN, screen.h - card.h - MARGIN)];
  const candidates = [
    [region.x0 - GAP - card.w, cy - card.h / 2 + GAP],
    [cx - card.w / 2, region.y1 + GAP],
    [region.x1 + GAP, cy - card.h / 2 + GAP],
    [cx - card.w / 2, region.y0 - GAP - card.h],
  ].map(fit);
  const covers = ([x, y]: number[]) =>
    Math.max(0, Math.min(x + card.w, region.x1) - Math.max(x, region.x0)) * Math.max(0, Math.min(y + card.h, region.y1) - Math.max(y, region.y0));
  const [x, y] = candidates.reduce((best, c) => (covers(c) < covers(best) ? c : best));
  // 矢印の根元は札の縁で区域の中心に一番近い点。先は中心の手前で止める
  const fromX = clamp(cx, x, x + card.w);
  const fromY = clamp(cy, y, y + card.h);
  const d = Math.hypot(cx - fromX, cy - fromY) || 1;
  const stop = Math.min(10, d / 2);
  return { x, y, fromX, fromY, toX: cx - ((cx - fromX) / d) * stop, toY: cy - ((cy - fromY) / d) * stop };
}

const MAX_ITEMS = 2;

/** 札の中身。1 種類 2 行 (種類 / 県と市町村) を MAX_ITEMS まで、残りは件数 */
export function warnCardHtml(f: WarnFocus): string {
  const rows = f.items
    .slice(0, MAX_ITEMS)
    .map((i) => `<div class="wc-item ${i.level}"><div class="wc-name">${esc(i.name)}</div><div class="wc-places">${esc(i.places)}</div></div>`);
  const rest = f.items.length - MAX_ITEMS;
  return rows.join("") + (rest > 0 ? `<div class="wc-rest">ほか ${rest} 件</div>` : "");
}
