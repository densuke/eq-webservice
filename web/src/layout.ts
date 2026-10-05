// 画面の構成の定義 (どの領域に、どの部品を、どの大きさで置くか)。並べるのは layout-dom.ts。
// DOM には触らない (テストから読み込めるように)。書式の説明は docs/ui-spec/layout-preview.html。

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

/** 大きさを親の向きに沿った CSS の flex にする */
export function flexOf(size: Size | undefined, pageColumn: boolean): string {
  const s = size ?? "fill";
  const fill = /^fill(?::(\d+(?:\.\d+)?))?$/.exec(s);
  if (fill) return pageColumn ? "0 1 auto" : `${fill[1] ?? 1} 1 0`;
  // CSS の既定 (flex: 0 1 auto) と同じ: 足りなければ縮む (詳細欄などはスクロールになる)
  if (s === "auto") return "0 1 auto";
  return `0 0 ${s}`;
}

const OVERLAYS_MAP_PC: LayoutNode["overlays"] = {
  // 別枠の右に天気の札の案内、その下にカウントダウン (別枠が隠れれば上に詰まる)
  "top-left": { flow: "column", items: [{ flow: "row", items: ["inset", "caption"] }, "countdown"] },
  "bottom-left": { flow: "column", items: ["legend"] },
  "bottom-right": { flow: "column", items: ["ogasawara", "clock"] },
  bottom: { flow: "column", items: ["toast", "hint"] },
};

/** いまの web の配置。横向きのスマホ (高さ 480 以下)・PC (幅 801 以上)・スマホ。上から順に最初に合うもの */
export const LAYOUTS: Layout[] = [
  {
    // 高さが足りないので、帯を地図の上に重ねる (EEW は 1 件ずつ巡回)。上部バーは細く (CSS)
    name: "landscape",
    when: { minWidth: 801, maxHeight: 480 },
    root: {
      dir: "column",
      children: [
        { slot: "topbar", size: "auto" },
        {
          box: "layout", dir: "row", size: "fill",
          children: [
            {
              slot: "main", size: "fill",
              overlays: {
                // 帯の下に別枠と凡例を横に並べる (縦に積むと高さが足りない)
                "top-left": { flow: "column", items: [{ slot: "banners", variant: "compact" }, { flow: "row", items: ["inset", "legend", "caption"] }, "countdown"] },
                "bottom-right": { flow: "column", items: ["ogasawara", "clock"] },
                bottom: { flow: "column", items: ["toast", "hint"] },
              },
            },
            {
              box: "side", dir: "column", size: "300px",
              children: [
                { slot: "settings", size: "auto" },
                { slot: "detail", size: "auto" },
                { slot: "history-head", size: "auto" },
                { slot: "history", size: "fill" },
                { slot: "notice", size: "fill" },
                { slot: "credit", size: "auto" },
              ],
            },
          ],
        },
      ],
    },
  },
  {
    name: "regular",
    when: { minWidth: 801 },
    root: {
      dir: "column",
      children: [
        { slot: "topbar", size: "auto" },
        { slot: "banners", size: "auto" },
        {
          box: "layout", dir: "row", size: "fill",
          children: [
            { slot: "main", size: "fill", overlays: OVERLAYS_MAP_PC },
            {
              box: "side", dir: "column", size: "380px",
              children: [
                { slot: "settings", size: "auto" },
                { slot: "detail", size: "auto" },
                { slot: "history-head", size: "auto" },
                { slot: "history", size: "fill" },
                { slot: "notice", size: "fill" },
                { slot: "credit", size: "auto" },
              ],
            },
          ],
        },
      ],
    },
  },
  {
    name: "compact",
    when: { maxWidth: 800 },
    scroll: "page",
    root: {
      dir: "column",
      children: [
        { slot: "topbar", size: "auto" },
        { slot: "banners", size: "auto" },
        {
          box: "layout", dir: "column", size: "auto",
          children: [
            {
              slot: "main", size: "60svh",
              overlays: {
                // 別枠の右に案内、下に凡例 (地図が細いと凡例が九州に重なるため)
                "top-left": { flow: "column", items: [{ flow: "row", items: ["inset", "caption"] }, "legend"] },
                // カウントダウンは時計の上に積む (横に並べると時計と重なる)
                "bottom-right": { flow: "column", items: ["ogasawara", "countdown", "clock"] },
                bottom: { flow: "column", items: ["toast", "hint"] },
              },
            },
            {
              box: "side", dir: "column", size: "auto",
              children: [
                { slot: "settings", size: "auto" },
                { slot: "detail", size: "auto" },
                { slot: "history-head", size: "auto" },
                { slot: "history", size: "auto" },
                { slot: "notice", size: "auto" },
                { slot: "credit", size: "auto" },
              ],
            },
          ],
        },
      ],
    },
  },
];
