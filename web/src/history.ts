// 履歴の再生: 選んだ地震の報を集め、当時の時刻で再生する準備をする (DOM には触らない)。
// 再生そのものはデモの仕組み (demo.ts の makePlan・demo-ui.ts) を使う。

import { GroupStore, type Group } from "./groups.ts";
import { type Place, sameQuake } from "./priority.ts";
import { groupPlace } from "./quakes.ts";
import { app } from "./state.ts";
import type { EqEvent } from "./types.ts";

/** 再生を始める位置: 最初の揺れ (発生時刻) のこの時間前 */
export const HISTORY_LEAD_MS = 10_000;
/** 次の報まで、これを超えて何も届かないときは、再生の時計を飛ばす (履歴の再生だけ) */
export const HISTORY_MAX_GAP_MS = 20_000;
/** 飛ばすとき、前の報のこの時間後から、次の報のこの時間前まで */
export const HISTORY_GAP_MARGIN_MS = 5_000;
/** 記録を取る範囲: 発生のこの時間前から */
const ARCHIVE_BEFORE_MS = 60_000;
/** 記録を取る範囲: 発生のこの時間後まで (各地の震度が出るまで) */
const ARCHIVE_AFTER_MS = 15 * 60_000;

/** 集めた報のうち、target と同じ地震の報だけを、届いた順に返す */
export function sameQuakeEvents(events: EqEvent[], target: Place): EqEvent[] {
  // 震度速報のように震源の無い報も同じ地震に入れるため、いったんグループにまとめてから判定する
  const scratch = new GroupStore();
  for (const e of events) if (e.kind === "quake" || e.kind === "eew") scratch.add(e);
  return scratch
    .list()
    .filter((g) => sameQuake(groupPlace(g), target))
    .flatMap((g) => g.events as EqEvent[])
    .sort((a, b) => a.received_at_ms - b.received_at_ms);
}

/**
 * 地震 g の報を集める。サーバの記録 (fetchArchive が null を返せば取れなかったもの) を優先し、
 * 取れなければ、いまブラウザが持っている分を使う
 */
export async function gatherEvents(g: Group, fetchArchive: (from: number, to: number) => Promise<EqEvent[] | null>): Promise<EqEvent[]> {
  const target = groupPlace(g);
  if (target.originMs == null) return [];
  const archived = await fetchArchive(Math.round(target.originMs - ARCHIVE_BEFORE_MS), Math.round(target.originMs + ARCHIVE_AFTER_MS)).catch(() => null);
  const found = sameQuakeEvents(archived ?? [], target);
  return found.length ? found : sameQuakeEvents(app.world.store.list().flatMap((x) => x.events as EqEvent[]), target);
}

/** サーバの記録 (`/api/archive`)。無い・取れないときは null */
export async function fetchArchive(from: number, to: number): Promise<EqEvent[] | null> {
  const res = await fetch(`api/archive?from=${from}&to=${to}`);
  return res.ok ? await res.json() : null;
}

/**
 * 再生の始まりの時刻。緊急地震速報があれば、その秒単位の発生時刻の HISTORY_LEAD_MS 前。
 * 無ければ最初の報が届いた時刻の HISTORY_LEAD_MS 前 (地震情報の発生時刻は分単位で、報はその 1〜2 分後に届くため)。報が無ければ null
 */
export function historyStart(events: EqEvent[]): number | null {
  const eew = events.flatMap((e) => (e.kind === "eew" && e.origin_time_ms != null ? [e.origin_time_ms] : []));
  if (eew.length) return Math.min(...eew) - HISTORY_LEAD_MS;
  return events.length ? Math.min(...events.map((e) => e.received_at_ms)) - HISTORY_LEAD_MS : null;
}

/** 報の再生位置 (時刻順) から、時計を飛ばす区間 [from, to) の再生位置を出す。報の間が HISTORY_MAX_GAP_MS ちょうどまでは飛ばさない */
export function skipRanges(ats: number[]): { from: number; to: number }[] {
  return ats.slice(1).flatMap((b, i) => {
    const a = ats[i];
    return b - a > HISTORY_MAX_GAP_MS ? [{ from: a + HISTORY_GAP_MARGIN_MS, to: b - HISTORY_GAP_MARGIN_MS }] : [];
  });
}
