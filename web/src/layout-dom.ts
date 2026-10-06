// レイアウトの定義 (layout.ts) どおりに、いまのページの要素を並べ直す。
// 部品の要素は index.html (と JapanMap が作る別枠) にあるものをそのまま使い、置き場所と大きさだけを決める。

import { type Layout, type LayoutNode, type Part, type Stack, type SubMapConfig, LAYOUTS, SUB_MAP, boxesOf, checkLayouts, chooseLayout, flexOf, slotsOf, withSubMapDefaults } from "./layout.ts";

/** 部品の名前 → 要素 (複数なら順に並べる) */
export const SLOTS: Record<string, string> = {
  topbar: "header.topbar",
  banners: "#eew-banner, #tsunami-banner, #warn-banner",
  main: "#map",
  "map-sub": "#map-sub",
  settings: "#settings-panel, #demo-panel",
  "eew-panel": "#eew-panel",
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

/** 容器の要素も最初に一度だけ探す (使わない定義の間に隠し置き場へ移しても、戻るときに見つかるように) */
const foundBoxes = new Map<string, HTMLElement | null>();
function boxElement(name: string): HTMLElement | null {
  if (!foundBoxes.has(name)) foundBoxes.set(name, document.querySelector<HTMLElement>(BOXES[name] ?? ""));
  return foundBoxes.get(name)!;
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
  if (spec.gap) box.style.gap = spec.gap;
  if (spec.pad) box.style.padding = spec.pad;
  if (spec.minHeight) {
    box.style.minHeight = spec.minHeight;
    // 中身が全部 hidden でも消えずに場所を取る (style.css の .ld-stack:not(:has(> :not([hidden]))) より inline が勝つ)
    box.style.display = "flex";
  }
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
  // 地図の中の他の重ね物 (画面外の地震の矢印など。同じ z-index) より下に置く
  host.prepend(layer);
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
  const box = node.box ? boxElement(node.box) : document.createElement("div");
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
  return [
    ...slotsOf(node).filter((n) => !SLOTS[n] || elements(n).length === 0).map((n) => `部品「${n}」`),
    ...boxesOf(node).filter((b) => !boxElement(b)).map((b) => `容器「${b}」`),
  ];
}

let current: Layout | null = null;
/** 使う定義 (はじめは組み込み。定義ファイルが読めたらそれ) */
let layouts: readonly Layout[] = LAYOUTS;
let subMap: SubMapConfig = SUB_MAP;

/** いま使うサブの地図の設定 (はじめは組み込み。定義ファイルが読めて正しければそれ) */
export function subMapConfig(): SubMapConfig {
  return subMap;
}

/** 画面の大きさに合う定義で並べる。前と同じ定義なら何もしない。
 *  部品や容器が見つからなければ、ログを出していまの並びのままにする (ページ全体を止めない) */
export function applyLayout(): void {
  const next = chooseLayout(layouts, window.innerWidth, window.innerHeight, new URLSearchParams(location.search).get("layout"));
  if (next === current) return;
  const lack = missing(next.root);
  if (lack.length) {
    console.error(`layout: ${next.name} で並べられない (${lack.join("・")} が無い)`);
    return;
  }
  current = next;
  // 定義が変わると、見せ方の段階も変わりうる (帯の巡回など)。描き直しを待たずに次の tick で反映される
  // 要素を新しい容器へ入れ直してから、前の容器と層 (もう空) を消す
  const old = made;
  made = [];
  document.body.dataset.layout = next.name;
  place(next.root, document.body, next.scroll === "page");
  // 定義に無い部品は隠し置き場へ (文書から外すと、document.querySelector で探すコードや ResizeObserver が対象を失う)
  const used = slotsOf(next.root);
  const unused = document.createElement("div");
  unused.className = "ld-unused";
  unused.hidden = true;
  for (const slot of Object.keys(SLOTS)) if (!used.includes(slot)) unused.append(...part(slot));
  // 使わない容器も (戻るときは place が付け直す)
  const usedBoxes = boxesOf(next.root);
  for (const b of Object.keys(BOXES)) {
    const el = boxElement(b);
    if (el && !usedBoxes.includes(b)) unused.append(el);
  }
  document.body.append(unused);
  made.push(unused);
  for (const el of old) el.remove();
}

/**
 * eq-server の定義ファイル (GET api/layout) を読み、正しければそれで並べ直す。
 * 読めない (サーバの無い静的な配信など)・正しくないときは、ログを出して組み込みの定義のまま (ページは止めない)
 */
export async function loadLayoutFile(): Promise<void> {
  try {
    const res = await fetch("api/layout", { cache: "no-store" });
    if (!res.ok) return;
    const data: unknown = await res.json();
    const errs = checkLayouts(data, Object.keys(SLOTS), Object.keys(BOXES));
    if (errs.length) {
      console.error(`layout: 定義ファイルが正しくないので、組み込みの定義を使う\n${errs.join("\n")}`);
      return;
    }
    const given = (data as { subMap?: Parameters<typeof withSubMapDefaults>[0] }).subMap;
    subMap = given ? withSubMapDefaults(given) : SUB_MAP;
    const next = (data as { layouts: Layout[] }).layouts;
    if (JSON.stringify(next) === JSON.stringify(layouts)) return;
    layouts = next;
    current = null;
    applyLayout();
  } catch (e) {
    console.error("layout: 定義ファイルを読めないので、組み込みの定義を使う", e);
  }
}
