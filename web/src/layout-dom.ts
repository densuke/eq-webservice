// レイアウトの定義 (layout.ts) どおりに、いまのページの要素を並べ直す。
// 部品の要素は index.html (と JapanMap が作る別枠) にあるものをそのまま使い、置き場所と大きさだけを決める。

import { type Layout, type LayoutNode, type Stack, LAYOUTS, flexOf, pickLayout } from "./layout.ts";

/** 部品の名前 → 要素 (複数なら順に並べる) */
export const SLOTS: Record<string, string> = {
  topbar: "header.topbar",
  banners: "#eew-banner, #tsunami-banner, #warn-banner",
  main: "#map",
  settings: "#settings-panel, #demo-panel",
  detail: "#detail",
  "history-head": ".list-head",
  history: "#list",
  notice: "#banner",
  credit: "footer.credit",
  // 地図の上の重ね物
  inset: ".inset-okinawa",
  ogasawara: ".inset-ogasawara",
  caption: "#weather-caption",
  countdown: "#countdown",
  legend: ".legend",
  clock: "#clock",
  toast: "#tour-toast",
  hint: "#sound-hint",
};

/** 容器に使う既存の要素 */
export const BOXES: Record<string, string> = { layout: "main.layout", side: "aside.side" };

/** 要素は最初に一度だけ探す (並べ直しの途中で文書から外れていても見つかるように) */
const found = new Map<string, HTMLElement[]>();
function elements(slot: string): HTMLElement[] {
  const sel = SLOTS[slot];
  if (!sel) throw new Error(`layout: 部品「${slot}」は無い`);
  if (!found.has(slot)) found.set(slot, [...document.querySelectorAll<HTMLElement>(sel)]);
  return found.get(slot)!;
}

function stack(spec: Stack, cls: string): HTMLElement {
  const box = document.createElement("div");
  box.className = cls;
  box.style.flexDirection = spec.flow;
  for (const item of spec.items) {
    if (typeof item === "string") box.append(...elements(item));
    else box.append(stack(item, "ld-stack"));
  }
  return box;
}

/** 前に作った容器と重ね物の層 (並べ直すたびに作り直す) */
let made: HTMLElement[] = [];

function overlays(host: HTMLElement, spec: LayoutNode["overlays"]): void {
  if (!spec) return;
  const layer = document.createElement("div");
  layer.className = "ld-layer";
  for (const [corner, s] of Object.entries(spec)) {
    const box = stack(s, `ld-corner ld-${corner}`);
    layer.append(box);
  }
  // 地図の中の他の重ね物 (画面外の地震の矢印など。同じ z-index) より下に置く
  host.prepend(layer);
  made.push(layer);
}

function place(node: LayoutNode, parent: HTMLElement, pageColumn: boolean): void {
  const flex = flexOf(node.size, pageColumn);
  if (node.slot) {
    const els = elements(node.slot);
    for (const el of els) {
      el.style.flex = flex;
      parent.append(el);
    }
    if (els[0]) overlays(els[0], node.overlays);
    return;
  }
  const box = node.box ? document.querySelector<HTMLElement>(BOXES[node.box] ?? "") : document.createElement("div");
  if (!box) throw new Error(`layout: 容器「${node.box}」は無い`);
  if (!node.box) made.push(box);
  box.classList.add("ld-box");
  box.style.flex = flex;
  box.style.flexDirection = node.dir ?? "column";
  parent.append(box);
  for (const ch of node.children ?? []) place(ch, box, pageColumn && (node.dir ?? "column") === "column");
  overlays(box, node.overlays);
}

/** 定義の中で、ページに見つからない部品と容器 (並べる前に確かめ、一つでもあれば並べ直さない) */
function missing(node: LayoutNode): string[] {
  const names = (s: Stack): string[] => s.items.flatMap((i) => (typeof i === "string" ? [i] : names(i)));
  const used = [...(node.slot ? [node.slot] : []), ...Object.values(node.overlays ?? {}).flatMap((s) => (s ? names(s) : []))];
  return [
    ...used.filter((n) => !SLOTS[n] || elements(n).length === 0).map((n) => `部品「${n}」`),
    ...(node.box && !document.querySelector(BOXES[node.box] ?? "") ? [`容器「${node.box}」`] : []),
    ...(node.children ?? []).flatMap(missing),
  ];
}

let current: Layout | null = null;

/** 画面の大きさに合う定義で並べる。前と同じ定義なら何もしない。
 *  部品や容器が見つからなければ、ログを出していまの並びのままにする (ページ全体を止めない) */
export function applyLayout(layouts: readonly Layout[] = LAYOUTS): void {
  const next = pickLayout(layouts, window.innerWidth, window.innerHeight);
  if (next === current) return;
  const lack = missing(next.root);
  if (lack.length) {
    console.error(`layout: ${next.name} で並べられない (${lack.join("・")} が無い)`);
    return;
  }
  current = next;
  // 要素を新しい容器へ入れ直してから、前の容器と層 (もう空) を消す
  const old = made;
  made = [];
  document.body.dataset.layout = next.name;
  place(next.root, document.body, next.scroll === "page");
  for (const el of old) el.remove();
}
