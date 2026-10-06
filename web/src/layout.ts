// 画面の構成の定義 (どの領域に、どの部品を、どの大きさで置くか) の型・選び方・確かめ方。並べるのは layout-dom.ts。
// 定義そのものは layout.json (組み込み) と、eq-server の定義ファイル (GET api/layout)。
// DOM には触らない (テストから読み込めるように)。書式の説明は docs/ui-spec/layout-system.html の「定義ファイル」。

import BUILTIN from "./layout.json" with { type: "json" };

/** "fill" (残りを分ける。"fill:2" は重み 2) / "auto" (部品の中身の大きさ。足りなければ縮む) / CSS の長さ ("380px"・"70%"・"60svh" など。縮まない) */
export type Size = string;

/** 部品の見せ方の段階を指定して置く (例: 帯を 1 件ずつ巡回する "compact")。部品の要素に data-variant として付く */
export interface Part {
  slot: string;
  variant: string;
}

/** 隅に積む重ね物。items は上から下 (左から右) の順 */
export interface Stack {
  flow: "row" | "column";
  items: (string | Part | Stack)[];
  /** 中身の間隔・内側の余白 (pad は 1〜4 個)・最小の高さ。CSS の長さ。minHeight は中身が全部隠れても場所を取る */
  gap?: string;
  pad?: string;
  minHeight?: string;
}

export type Corner = "top-left" | "top-right" | "bottom-left" | "bottom-right" | "top" | "bottom";

export interface LayoutNode {
  /** 部品 (SLOTS の名前)。無ければ容器 */
  slot?: string;
  /** 部品の見せ方の段階 (Part と同じ) */
  variant?: string;
  /** 容器の向き */
  dir?: "row" | "column";
  /** 容器に使う既存の要素 (BOXES の名前)。無ければ div を作る */
  box?: string;
  size?: Size;
  children?: LayoutNode[];
  overlays?: Partial<Record<Corner, Stack>>;
}

export interface Layout {
  name: string;
  /** 選ぶ条件 (上から順に最初に合うもの) */
  when?: { minWidth?: number; maxWidth?: number; minHeight?: number; maxHeight?: number };
  /** ページ全体がスクロールする (縦に積む中の "fill" は "auto" として扱う) */
  scroll?: "page";
  root: LayoutNode;
}

/** 画面の大きさで定義を選ぶ。どれにも合わなければ先頭 */
export function pickLayout(layouts: readonly Layout[], width: number, height: number): Layout {
  const ok = ({ minWidth, maxWidth, minHeight, maxHeight }: NonNullable<Layout["when"]>) =>
    (minWidth == null || width >= minWidth) && (maxWidth == null || width <= maxWidth) && (minHeight == null || height >= minHeight) && (maxHeight == null || height <= maxHeight);
  return layouts.find((l) => ok(l.when ?? {})) ?? layouts[0];
}

/** 定義の中に置かれた部品の名前を、重ね物と入れ子の積みを含めて出てきた順に返す (重複もそのまま) */
export function slotsOf(node: LayoutNode): string[] {
  const fromStack = (s: Stack): string[] => s.items.flatMap((i) => (typeof i === "string" ? [i] : "slot" in i ? [i.slot] : fromStack(i)));
  return [
    ...(node.slot ? [node.slot] : []),
    ...Object.values(node.overlays ?? {}).flatMap((s) => (s ? fromStack(s) : [])),
    ...(node.children ?? []).flatMap(slotsOf),
  ];
}

/** 省けない部品 (これ以外は定義で置かなくてよい) */
export const REQUIRED_SLOTS: readonly string[] = ["main"];

/** 大きさを親の向きに沿った CSS の flex にする */
export function flexOf(size: Size | undefined, pageColumn: boolean): string {
  const s = size ?? "fill";
  const fill = /^fill(?::(\d+(?:\.\d+)?))?$/.exec(s);
  if (fill) return pageColumn ? "0 1 auto" : `${fill[1] ?? 1} 1 0`;
  // CSS の既定 (flex: 0 1 auto) と同じ: 足りなければ縮む (詳細欄などはスクロールになる)
  if (s === "auto") return "0 1 auto";
  return `0 0 ${s}`;
}

/** 組み込みの定義 (layout.json)。eq-server の定義ファイル (GET api/layout) が読めない・正しくないときにも使う */
export const LAYOUTS: readonly Layout[] = (BUILTIN as unknown as { layouts: Layout[] }).layouts;

const CORNERS: readonly string[] = ["top-left", "top-right", "bottom-left", "bottom-right", "top", "bottom"];
const WHEN: readonly string[] = ["minWidth", "maxWidth", "minHeight", "maxHeight"];
/** fill・fill:n・auto・数 + 単位・calc() などの CSS の関数 */
/** 0 か、数 + 単位 */
const LENGTH = /^(?:0|\d+(?:\.\d+)?(?:px|em|rem|%|vw|vh|svh|dvh))$/;
const SIZE = /^(?:fill(?::\d+(?:\.\d+)?)?|auto|\d+(?:\.\d+)?(?:px|%|vw|vh|svw|svh|dvw|dvh|lvw|lvh|em|rem)|(?:calc|clamp|min|max)\([\w\s.,%+*/()-]*\))$/;

/**
 * 定義ファイル ({ version: 1, layouts: [...] }) を確かめ、正しくないところを返す (正しければ空)。
 * 部品の名前は slots、容器の名前は boxes にあるものだけ。各定義は部品を高々 1 回置き、REQUIRED_SLOTS は必ず置く。
 * 知らないキーも誤りにする (書き間違いに気づけるように)。説明には "note" をどこにでも書ける
 */
export function checkLayouts(data: unknown, slots: readonly string[], boxes: readonly string[]): string[] {
  const errs: string[] = [];
  const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
  const keys = (v: Record<string, unknown>, at: string, allowed: string[]) => {
    for (const k of Object.keys(v)) if (k !== "note" && !allowed.includes(k)) errs.push(`${at}: 知らないキー「${k}」`);
  };
  const part = (name: unknown, at: string, placed: string[]) => {
    if (typeof name === "string" && slots.includes(name)) placed.push(name);
    else errs.push(`${at}: 知らない部品 ${JSON.stringify(name)}`);
  };
  const stack = (s: unknown, at: string, placed: string[]): void => {
    if (!isObj(s)) return void errs.push(`${at}: 重ね方がオブジェクトでない`);
    keys(s, at, ["flow", "items", "gap", "pad", "minHeight"]);
    for (const k of ["gap", "minHeight"]) if (s[k] !== undefined && !(typeof s[k] === "string" && LENGTH.test(s[k]))) errs.push(`${at}: ${k} ${JSON.stringify(s[k])} は使えない`);
    if (s.pad !== undefined) {
      const parts = typeof s.pad === "string" ? s.pad.split(" ") : [];
      if (parts.length < 1 || parts.length > 4 || !parts.every((p) => LENGTH.test(p))) errs.push(`${at}: pad ${JSON.stringify(s.pad)} は使えない`);
    }
    if (s.flow !== "row" && s.flow !== "column") errs.push(`${at}: flow は "row" か "column"`);
    if (!Array.isArray(s.items)) return void errs.push(`${at}: items が配列でない`);
    s.items.forEach((it, i) => {
      const a = `${at}.items[${i}]`;
      if (typeof it === "string") part(it, a, placed);
      else if (isObj(it) && "slot" in it) {
        keys(it, a, ["slot", "variant"]);
        part(it.slot, a, placed);
        if (typeof it.variant !== "string") errs.push(`${a}: variant が文字列でない`);
      } else stack(it, a, placed);
    });
  };
  const node = (n: unknown, at: string, placed: string[]): void => {
    if (!isObj(n)) return void errs.push(`${at}: オブジェクトでない`);
    keys(n, at, ["slot", "variant", "dir", "box", "size", "children", "overlays"]);
    if (n.size !== undefined && !(typeof n.size === "string" && SIZE.test(n.size))) errs.push(`${at}: 大きさ ${JSON.stringify(n.size)} は使えない`);
    if (n.slot !== undefined) {
      part(n.slot, at, placed);
      if (n.variant !== undefined && typeof n.variant !== "string") errs.push(`${at}: variant が文字列でない`);
      if (n.children !== undefined || n.box !== undefined) errs.push(`${at}: 部品 (slot) に children・box は付けられない`);
    } else {
      if (n.dir !== undefined && n.dir !== "row" && n.dir !== "column") errs.push(`${at}: dir は "row" か "column"`);
      if (n.box !== undefined && !(typeof n.box === "string" && boxes.includes(n.box))) errs.push(`${at}: 知らない容器 ${JSON.stringify(n.box)}`);
      if (!Array.isArray(n.children)) errs.push(`${at}: 容器に children (配列) が無い`);
      else n.children.forEach((c, i) => node(c, `${at}.children[${i}]`, placed));
    }
    if (n.overlays === undefined) return;
    if (!isObj(n.overlays)) return void errs.push(`${at}: overlays がオブジェクトでない`);
    for (const [corner, s] of Object.entries(n.overlays)) {
      if (CORNERS.includes(corner)) stack(s, `${at}.overlays.${corner}`, placed);
      else errs.push(`${at}: 知らない隅「${corner}」`);
    }
  };

  if (!isObj(data)) return ["定義ファイルが JSON のオブジェクトでない"];
  keys(data, "定義ファイル", ["version", "layouts"]);
  if (data.version !== 1) errs.push(`version は 1 (いまは ${JSON.stringify(data.version)})`);
  if (!Array.isArray(data.layouts) || data.layouts.length === 0) return [...errs, "layouts が空か、配列でない"];
  data.layouts.forEach((l, i) => {
    const at = `layouts[${i}]`;
    if (!isObj(l)) return void errs.push(`${at}: オブジェクトでない`);
    const name = `${at} (${String(l.name)})`;
    keys(l, name, ["name", "when", "scroll", "root"]);
    if (typeof l.name !== "string" || !l.name) errs.push(`${at}: name が無い`);
    if (l.when !== undefined) {
      if (!isObj(l.when)) errs.push(`${name}: when がオブジェクトでない`);
      else for (const [k, v] of Object.entries(l.when)) if (!WHEN.includes(k) || typeof v !== "number") errs.push(`${name}: when.${k} は使えない (${WHEN.join("・")} に数)`);
    }
    if (l.scroll !== undefined && l.scroll !== "page") errs.push(`${name}: scroll は "page" だけ`);
    const placed: string[] = [];
    node(l.root, `${name}.root`, placed);
    for (const s of slots) {
      const n = placed.filter((p) => p === s).length;
      if (n > 1) errs.push(`${name}: 部品「${s}」を ${n} 回置いている`);
      if (n === 0 && REQUIRED_SLOTS.includes(s)) errs.push(`${name}: 部品「${s}」を置いていない`);
    }
  });
  return errs;
}
