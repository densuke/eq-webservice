// デモモード: 場面の情報を、発生時刻を「今」にずらしてブラウザの中だけで再生する。
// ずらし方はサーバの replay と同じ: 間隔は詰め (最大 MAX_GAP_MS)、発表時刻はそれぞれ「受け取った時刻」に、
// 発生時刻は最初の報に合わせて固定する (同じ地震の続報が別の地震に見えないように)。

import type { EqEvent } from "./types.ts";

export const MAX_GAP_MS = 8000;
const JST_MS = 9 * 3600_000;

export interface ScenarioSummary {
  id: string;
  name: string;
  description: string;
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
  return "issued_at" in e ? parseJst(e.issued_at) : null;
}

/** 1 件をずらす。issuedDelta は発表時刻、originDelta は発生時刻・到達予想時刻に足す */
function shift(e: EqEvent, issuedDelta: number, originDelta: number, run: number): EqEvent {
  const id = `${e.id}#demo${run}`;
  const ms = (v: number | null) => (v == null ? null : v + originDelta);
  switch (e.kind) {
    case "quake":
      return { ...e, id, source: "demo", issued_at: shiftJst(e.issued_at, issuedDelta), origin_time: shiftJst(e.origin_time, originDelta), origin_time_ms: ms(e.origin_time_ms) };
    case "eew":
      return {
        ...e,
        id,
        source: "demo",
        event_id: `${e.event_id}#demo${run}`,
        issued_at: shiftJst(e.issued_at, issuedDelta),
        origin_time: e.origin_time && shiftJst(e.origin_time, originDelta),
        origin_time_ms: ms(e.origin_time_ms),
        areas: e.areas.map((a) => ({ ...a, arrival_time: a.arrival_time && shiftJst(a.arrival_time, originDelta) })),
      };
    case "tsunami":
      return { ...e, id, source: "demo", issued_at: shiftJst(e.issued_at, issuedDelta) };
    default:
      return { ...e, id, source: "demo" };
  }
}

/** 再生の予定: at (再生開始からのミリ秒) と、その時刻に届いたことにする情報。run は再生ごとの番号 (ID の重複を避ける) */
export function schedule(events: EqEvent[], now: number, run: number): { at: number; event: EqEvent }[] {
  const out: { at: number; event: EqEvent }[] = [];
  let at = 0;
  let prev: number | null = null;
  let originDelta: number | null = null;
  for (const e of events) {
    const rec = issuedMs(e);
    if (prev != null && rec != null && rec > prev) at += Math.min(rec - prev, MAX_GAP_MS);
    if (rec != null) prev = rec;
    const issuedDelta = rec == null ? 0 : now + at - rec;
    originDelta ??= rec == null ? null : issuedDelta;
    out.push({ at, event: { ...shift(e, issuedDelta, originDelta ?? issuedDelta, run), received_at_ms: now + at } });
  }
  return out;
}
