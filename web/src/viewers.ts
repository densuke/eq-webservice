// YouTube ライブの同時視聴者数。ヘッダーの隅に「同接 N 人」を出す (GET /api/viewers を 1 分ごとに取る)。
// 値が無い・古い・取れないときは出さない。api が無い静的配信でも何も起きない。

// (dom.ts は読み込むだけで document を触るので、純粋な関数のテストのために使わない)

const EVERY_MS = 60_000;
/** サーバの取得が止まったとき、古い数を出し続けない */
const STALE_MS = 5 * 60_000;

/** 表示する文。出さないときは null (応答の形が違うときも null) */
export function viewersLabel(data: unknown, now: number): string | null {
  if (typeof data !== "object" || data === null) return null;
  const { viewers, updated_ms } = data as { viewers?: unknown; updated_ms?: unknown };
  if (typeof viewers !== "number" || !Number.isInteger(viewers) || viewers < 0) return null;
  if (typeof updated_ms !== "number" || now - updated_ms > STALE_MS) return null;
  return `同接 ${viewers.toLocaleString("ja-JP")} 人`;
}

/** サーバが無効 (まだ一度も取っていない。updated_ms が 0) なら、以後取りに行かない */
export function isDisabled(data: unknown): boolean {
  return typeof data === "object" && data !== null && (data as { updated_ms?: unknown }).updated_ms === 0;
}

/** 取りに行って表示を更新する。もう取りに行かなくてよいときは true */
async function refresh(): Promise<boolean> {
  const el = document.getElementById("viewers");
  if (!el) return true;
  let label: string | null = null;
  let off = false;
  try {
    const res = await fetch("api/viewers");
    if (res.ok) {
      const data: unknown = await res.json();
      off = isDisabled(data);
      label = viewersLabel(data, Date.now());
    }
  } catch {
    // 取れなければ出さない
  }
  el.hidden = label === null;
  el.textContent = label ?? "";
  return off;
}

export function startViewers(): void {
  const timer = window.setInterval(() => void refresh().then((off) => off && clearInterval(timer)), EVERY_MS);
  void refresh().then((off) => off && clearInterval(timer));
}
