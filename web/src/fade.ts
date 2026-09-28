// 震度の塗り分けを時間とともに薄くする。

/** 発生からこの時間ははっきり表示する (自動カメラが寄っている時間と同じ) */
export const FULL_MS = 10 * 60_000;
/** この時間で消える */
export const GONE_MS = 60 * 60_000;

/** 発生からの経過時間に対する濃さ (1 = はっきり, 0 = 消える) */
export function fadeOpacity(ageMs: number): number {
  if (ageMs <= FULL_MS) return 1;
  if (ageMs >= GONE_MS) return 0;
  return 1 - (ageMs - FULL_MS) / (GONE_MS - FULL_MS);
}
