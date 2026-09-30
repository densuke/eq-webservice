// 履歴の再生: 選んだ地震の報を集め、当時の時刻で再生する準備をする (DOM には触らない)。
// 再生そのものはデモの仕組み (demo.ts の makePlan・demo-ui.ts) を使う。

import { GroupStore, type Group } from "./groups.ts";
import { type Place, sameQuake } from "./priority.ts";
import { groupPlace } from "./quakes.ts";
import { app } from "./state.ts";
import type { EqEvent } from "./types.ts";

/** 再生を始める位置: 最初の揺れ (発生時刻) のこの時間前 */
export const HISTORY_LEAD_MS = 10_000;
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

/** 再生の始まりの時刻: 発生時刻の HISTORY_LEAD_MS 前。緊急地震速報の秒単位の発生時刻を優先する (地震情報のは分単位)。分からなければ null */
export function historyStart(events: EqEvent[]): number | null {
  const origins = (kind: "eew" | "quake") =>
    events.flatMap((e) => (e.kind === kind && e.origin_time_ms != null ? [e.origin_time_ms] : []));
  const o = origins("eew").length ? origins("eew") : origins("quake");
  return o.length ? Math.min(...o) - HISTORY_LEAD_MS : null;
}
