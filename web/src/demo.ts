// デモモード: 場面の情報を、デモ専用の時計に合わせてブラウザの中だけで再生する。
// 再生位置 (ミリ秒) と当時の時刻の対応表を作る。波を描いている間は実時間のまま、それ以外の長い間は詰める (最大 MAX_GAP_MS。
// その間は時計が早く進む)。記録の場面は時刻をずらさず (時計は当時の日時)、架空の場面は再生の始まりが「今」になるようにずらす。

import { WAVE_MAX_SEC } from "./waves.ts";
import type { EqEvent } from "./types.ts";

const MAX_GAP_MS = 8000;
const JST_MS = 9 * 3600_000;

export interface ScenarioSummary {
  id: string;
  name: string;
  description: string;
  /** 過去の地震の記録を再生する場面の出典 */
  source?: string;
}

export interface Scenario extends ScenarioSummary {
  events: EqEvent[];
}

/** "YYYY/MM/DD HH:MM:SS" (日本時間) の epoch ミリ秒。読めなければ null */
export function parseJst(s: string): number | null {
  const m = /^(\d{4})\/(\d{2})\/(\d{2}) (\d{2}):(\d{2}):(\d{2})/.exec(s);
  if (!m) return null;
  const [y, mo, d, h, mi, se] = m.slice(1).map(Number);
  return Date.UTC(y, mo - 1, d, h, mi, se) - JST_MS;
}

function formatJst(ms: number): string {
  const d = new Date(ms + JST_MS);
  const p2 = (n: number) => String(n).padStart(2, "0");
  return `${d.getUTCFullYear()}/${p2(d.getUTCMonth() + 1)}/${p2(d.getUTCDate())} ${p2(d.getUTCHours())}:${p2(d.getUTCMinutes())}:${p2(d.getUTCSeconds())}`;
}

export function shiftJst(s: string, delta: number): string {
  const ms = parseJst(s);
  return ms == null ? s : formatJst(ms + delta);
}

function issuedMs(e: EqEvent): number | null {
  if (e.kind === "userquake") return parseJst(e.updated_at);
  return "issued_at" in e ? parseJst(e.issued_at) : null;
}

/** 1 件をずらす (ID には再生ごとの番号を付ける) */
function shift(e: EqEvent, delta: number, run: number): EqEvent {
  const id = `${e.id}#demo${run}`;
  const ms = (v: number | null) => (v == null ? null : v + delta);
  const t = (v: string) => shiftJst(v, delta);
  switch (e.kind) {
    case "quake":
      return { ...e, id, source: "demo", issued_at: t(e.issued_at), origin_time: t(e.origin_time), origin_time_ms: ms(e.origin_time_ms) };
    case "eew":
      return {
        ...e,
        id,
        source: "demo",
        event_id: `${e.event_id}#demo${run}`,
        issued_at: t(e.issued_at),
        origin_time: e.origin_time && t(e.origin_time),
        origin_time_ms: ms(e.origin_time_ms),
        areas: e.areas.map((a) => ({ ...a, arrival_time: a.arrival_time && t(a.arrival_time) })),
      };
    case "tsunami":
      return { ...e, id, source: "demo", issued_at: t(e.issued_at) };
    case "userquake":
      return { ...e, id, source: "demo", started_at: t(e.started_at), updated_at: t(e.updated_at) };
    default:
      return { ...e, id, source: "demo" };
  }
}

/** 再生を始める位置: 最初の地震 (無ければ最初の報) のこの時間前から */
const LEAD_MS = 5000;

export interface Plan {
  /** at は再生位置 (ミリ秒)。時刻順 */
  events: { at: number; event: EqEvent }[];
  /** 最後の報の再生位置 */
  end: number;
  /** 再生位置 -> そのときの時刻 (epoch ミリ秒) */
  toReal(pos: number): number;
}

/** [実時刻, 再生位置] の点を結んだ折れ線で写す (範囲の外は 1:1) */
function interpolate(points: [number, number][], x: number, from: 0 | 1): number {
  const to = from === 0 ? 1 : 0;
  const i = points.findIndex((p) => p[from] > x);
  const [a, b] = i === -1 ? [points[points.length - 1], null] : i === 0 ? [points[0], null] : [points[i - 1], points[i]];
  if (!b || b[from] === a[from]) return a[to] + (x - a[from]);
  return a[to] + ((x - a[from]) * (b[to] - a[to])) / (b[from] - a[from]);
}

/**
 * 再生の計画を作る。run は再生ごとの番号 (ID の重複を避ける)。
 * startAt を渡すと再生の始まりがその時刻になるようにずらす (架空の場面)。渡さなければ当時の時刻のまま (記録の場面)。
 * start は再生の始まりにする当時の時刻 (epoch ミリ秒)。渡さなければ最初の地震 (報) の LEAD_MS 前 (履歴の再生が使う)
 */
export function makePlan(events: EqEvent[], run: number, startAt?: number, startOverride?: number): Plan {
  const recs = events.map(issuedMs);
  const origins = events.map((e) => ("origin_time_ms" in e ? e.origin_time_ms : null)).filter((o): o is number => o != null);
  const known = recs.filter((r): r is number => r != null);
  const first = Math.min(...origins, ...known);
  const start = startOverride ?? ((Number.isFinite(first) ? first : 0) - LEAD_MS);
  const delta = startAt == null ? 0 : startAt - start;
  // 波を描いている間 (発生から WAVE_MAX_SEC)。重なりはまとめる
  const windows = [...origins]
    .sort((x, y) => x - y)
    .map((o): [number, number] => [o, o + WAVE_MAX_SEC * 1000])
    .reduce<[number, number][]>((acc, w) => {
      const last = acc[acc.length - 1];
      if (last && w[0] <= last[1]) last[1] = Math.max(last[1], w[1]);
      else acc.push([...w]);
      return acc;
    }, []);
  const points: [number, number][] = [[start, 0]];
  let prev = start;
  for (const rec of known) {
    if (rec <= prev) continue;
    // 間を、波を描いている部分 (実時間) と何も無い部分 (まとめて MAX_GAP_MS までに詰める) に分ける
    const cuts = [prev, ...windows.flat().filter((c) => c > prev && c < rec), rec];
    const pieces = cuts.slice(1).map((b, i) => ({ a: cuts[i], b, wave: windows.some(([w0, w1]) => w0 <= cuts[i] && b <= w1) }));
    const quiet = pieces.filter((x) => !x.wave).reduce((sum, x) => sum + (x.b - x.a), 0);
    const k = quiet > 0 ? Math.min(quiet, MAX_GAP_MS) / quiet : 1;
    for (const x of pieces) {
      const pos = points[points.length - 1][1] + (x.b - x.a) * (x.wave ? 1 : k);
      points.push([x.b, pos]);
    }
    prev = rec;
  }
  const toPos = (real: number) => interpolate(points, real, 0);
  // 受け取った時刻は発表時刻 (デモの時計で届く時刻) にする
  const planned = events.map((e, i) => ({ at: Math.round(toPos(recs[i] ?? start)), event: { ...shift(e, delta, run), received_at_ms: (recs[i] ?? start) + delta } }));
  return {
    events: planned,
    end: planned.reduce((m, x) => Math.max(m, x.at), 0),
    toReal: (pos) => interpolate(points, pos, 1) + delta,
  };
}
