// 天気の札の間引き: 画面が狭くて札どうし (や凡例など) が重なるとき、優先の低い札を出さない。

import type { Box } from "./camera.ts";

const overlap = (a: Box, b: Box) => a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1;

/** 札の四角 (優先の高い順) のうち出すもの。blockers (凡例・別枠・案内・時計) か、先に出すと決めた札と重なる札は出さない */
export function thinBoxes(boxes: Box[], blockers: Box[]): boolean[] {
  const placed: Box[] = [...blockers];
  return boxes.map((b) => {
    const ok = !placed.some((p) => overlap(b, p));
    if (ok) placed.push(b);
    return ok;
  });
}

const box = (e: Element): Box => {
  const r = e.getBoundingClientRect();
  return { x0: r.left, y0: r.top, x1: r.right, y1: r.bottom };
};

/** 札の層 (子は優先の高い順) の札を、重なるものから間引く (間引いた札は thin クラスで札だけ隠し、点は残す) */
export function thinCityLayer(layer: Element, blockers: Element[]): void {
  const cities = [...layer.children];
  for (const g of cities) g.classList.remove("thin");
  const keep = thinBoxes(
    cities.map((g) => box(g.querySelector(".city-box")!)),
    blockers.map(box),
  );
  cities.forEach((g, i) => g.classList.toggle("thin", !keep[i]));
}
