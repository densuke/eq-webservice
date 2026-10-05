// レイアウトの定義 (layout.ts) どおりに、いまのページの要素を並べ直す。
// 部品の要素は index.html (と JapanMap が作る別枠) にあるものをそのまま使い、置き場所と大きさだけを決める。

import { type Layout, type LayoutNode, type Part, type Stack, LAYOUTS, flexOf, pickLayout } from "./layout.ts";

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

/** 部品の要素を取り出し、見せ方の段階を付け直す (前の定義の段階は消す) */
function part(slot: string, variant?: string): HTMLElement[] {
  const els = elements(slot);
  for (const el of els) {
    if (variant) el.dataset.variant = variant;
    else delete el.dataset.variant;
  }
  return els;
}

function stack(spec: Stack, cls: string): HTMLElement {
  const box = document.createElement("div");
  box.className = cls;
  box.style.flexDirection = spec.flow;
  for (const item of spec.items) {
    if (typeof item === "string") box.append(...part(item));
    else if ("slot" in item) box.append(...part((item as Part).slot, (item as Part).variant));
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
  host.append(layer);
  made.push(layer);
}

function place(node: LayoutNode, parent: HTMLElement, pageColumn: boolean): void {
  const flex = flexOf(node.size, pageColumn);
  if (node.slot) {
    const els = part(node.slot, node.variant);
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

let current: Layout | null = null;

/** 画面の大きさに合う定義で並べる。前と同じ定義なら何もしない */
export function applyLayout(layouts: readonly Layout[] = LAYOUTS): void {
  const next = pickLayout(layouts, window.innerWidth, window.innerHeight);
  if (next === current) return;
  current = next;
  // 定義が変わると、見せ方の段階も変わりうる (帯の巡回など)。描き直しを待たずに次の tick で反映される
  // 要素を新しい容器へ入れ直してから、前の容器と層 (もう空) を消す
  const old = made;
  made = [];
  document.body.dataset.layout = next.name;
  place(next.root, document.body, next.scroll === "page");
  for (const el of old) el.remove();
}
