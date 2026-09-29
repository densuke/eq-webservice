// 配信 (?broadcast=1&audio=mixer) で、ページが eq-server に送る音の知らせ (docs/broadcast-v2.md の W1)。
// ページは音を鳴らさず、この JSON を window.eqBroadcast に渡す。鳴らす側 (eq-server の mixer) はこの形だけを知っている。

import type { AlertLevel } from "./alert.ts";

/** sound.ts の play() が鳴らせるもの全部 (AlertLevel のほか、波の刻みと揺れの報告) */
export type AlertSound = AlertLevel | "pip" | "feel";

export type Notice =
  | { type: "bgm"; play: true; volume: number }
  | { type: "bgm"; play: false }
  | { type: "alert"; level: AlertSound };

/** BGM の状態の知らせ。平時で BGM が ON なら流す (音量は 0〜1)、そうでなければ止める */
export function bgmNotice(wanted: boolean, volumePercent: number): Notice {
  if (!wanted) return { type: "bgm", play: false };
  return { type: "bgm", play: true, volume: Math.min(1, Math.max(0, volumePercent / 100)) };
}

/** 前に送った知らせと同じなら null (毎 tick 同じ知らせを送らない) */
export function changed(prev: Notice | null, next: Notice): Notice | null {
  return prev != null && JSON.stringify(prev) === JSON.stringify(next) ? null : next;
}

/** BGM が鳴っている扱いか。mixer はページで鳴らさないので、流す知らせを出している間 (wanted) を鳴っているとする */
export function isPlaying(mixer: boolean, wanted: boolean, paused: boolean): boolean {
  return mixer ? wanted : !paused;
}
