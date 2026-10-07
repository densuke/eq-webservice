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

async function refresh(): Promise<void> {
  const el = document.getElementById("viewers");
  if (!el) return;
  let label: string | null = null;
  try {
    const res = await fetch("api/viewers");
    if (res.ok) label = viewersLabel(await res.json(), Date.now());
  } catch {
    // 取れなければ出さない
  }
  el.hidden = label === null;
  el.textContent = label ?? "";
}

export function startViewers(): void {
  void refresh();
  window.setInterval(() => void refresh(), EVERY_MS);
}
