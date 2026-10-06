// DOM を扱うもの: 要素の取得と地図。

import { JapanMap } from "./map.ts";

export const $ = <T extends HTMLElement>(sel: string) => document.querySelector(sel) as T;
/** 操作の言い方: 指で操作する端末 (ポインタを合わせられない) は「タップ」、それ以外は「クリック」 */
export const tapWord = matchMedia("(hover: none) and (pointer: coarse)").matches ? "タップ" : "クリック";
export const map = new JapanMap($("#map"));
/** 右上のサブの地図 (試験定義 trial)。別枠・手の操作・矢印・帯の見張りは要らず、いつも区域と観測点で細かく描く */
export const subMap = new JapanMap($("#map-sub"), { insets: false, panZoom: false, offscreen: false, bandWatch: false, alwaysDetail: true });
