// DOM を扱うもの: 要素の取得と地図。

import { JapanMap } from "./map.ts";

export const $ = <T extends HTMLElement>(sel: string) => document.querySelector(sel) as T;
export const map = new JapanMap($("#map"));
