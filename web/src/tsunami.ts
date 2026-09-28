// 現在の津波予報。552 は毎回「今出ている予報の全体」なので、最新の 1 件で置き換え、解除で消す。

import type { AlertLevel } from "./alert.ts";
import type { TsunamiArea, TsunamiEvent } from "./types.ts";

const RANK: Record<TsunamiArea["grade"], number> = { unknown: 0, watch: 1, warning: 2, major_warning: 3 };

/** 受け取った最新の予報 (解除を含む) を返す。遅れて届いた古い予報は無視する */
export function latestTsunami(cur: TsunamiEvent | null, e: TsunamiEvent): TsunamiEvent {
  return cur && e.issued_at < cur.issued_at ? cur : e;
}

/** 発表中の予報区。解除されていれば空 */
export function activeAreas(t: TsunamiEvent | null): TsunamiArea[] {
  return !t || t.cancelled ? [] : t.areas;
}

export function maxRank(areas: TsunamiArea[]): number {
  return Math.max(0, ...areas.map((a) => RANK[a.grade]));
}

/** 予報が出た・等級が上がったときだけ鳴らす (続報のたびには鳴らさない) */
export function tsunamiAlert(prev: TsunamiArea[], next: TsunamiArea[]): AlertLevel | null {
  const r = maxRank(next);
  if (r <= maxRank(prev)) return null;
  return r >= RANK.warning ? "strong" : r === RANK.watch ? "medium" : null;
}
