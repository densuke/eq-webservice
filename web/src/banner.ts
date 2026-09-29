// 平時のバナー (案内・お知らせ)。右側の履歴の下半分に、サーバに置いた画像・テキストを順に出す。地震の表示の間は隠す。
// 置いた内容は文字は文字、画像は画像としてだけ出す (HTML として挿入しない)。

import { $ } from "./dom.ts";

interface Banner {
  image: string | null;
  text: string | null;
  link: string | null;
}

let items: Banner[] = [];
let intervalMs = 20_000;
let index = 0;
let shownAt = 0;
let rendered: Banner | null = null;

/** 一覧を取り直す (サーバは毎回ディレクトリを読み直すので、差し替えもここで入る) */
export async function loadBanners(): Promise<void> {
  const res = await fetch("api/banners");
  if (!res.ok) return;
  const data: { interval_sec: number; items: Banner[] } = await res.json();
  items = data.items;
  intervalMs = data.interval_sec * 1000;
  if (index >= items.length) index = 0;
}

/** 画面の更新 (tick) ごと: 平時なら出して一定の間隔で切り替え、そうでなければ隠す */
export function updateBanner(quiet: boolean): void {
  const box = $("#banner");
  const show = quiet && items.length > 0;
  box.hidden = !show;
  if (!show) return;
  const t = Date.now();
  if (rendered && t - shownAt >= intervalMs) {
    index = (index + 1) % items.length;
    // 一巡したら一覧を取り直す
    if (index === 0) void loadBanners().catch(() => {});
  }
  const b = items[index];
  if (b && b !== rendered) {
    render(box, b);
    rendered = b;
    shownAt = t;
  }
}

function render(box: HTMLElement, b: Banner): void {
  const wrap = b.link ? document.createElement("a") : document.createElement("div");
  wrap.className = "banner-item";
  if (b.link && wrap instanceof HTMLAnchorElement) {
    wrap.href = b.link;
    wrap.target = "_blank";
    wrap.rel = "noopener noreferrer";
  }
  if (b.image) {
    const img = document.createElement("img");
    img.src = b.image;
    img.alt = b.text ?? "";
    wrap.append(img);
  }
  if (b.text) {
    const p = document.createElement("p");
    p.textContent = b.text;
    wrap.append(p);
  }
  box.replaceChildren(wrap);
}
