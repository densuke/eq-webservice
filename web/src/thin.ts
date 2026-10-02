// 天気の札の置き場所: 札どうし (や凡例など) が重なるとき、優先の低い札を点の周りの空きへ逃がし、引き出し線でつなぐ。
// どこにも置けない札だけ出さない。座標は画面のピクセル。

import type { Box } from "./camera.ts";

type Pt = [number, number];

/** 札を点から離す距離 (札の縁から点まで) の候補 */
const GAPS = [14, 30, 48];
/** 点の周りの 8 方向 (上・下・左・右を先に) */
const DIRS: Pt[] = [[0, -1], [0, 1], [-1, 0], [1, 0], [-1, -1], [1, -1], [-1, 1], [1, 1]];
/** 他の都市の点を札で覆わないための余白 */
const DOT_R = 5;

export interface Placement {
  /** 元の位置からの動き。動かしていなければ 0 */
  dx: number;
  dy: number;
  /** 動かした札だけ: 点から引き出し線の先 (札の縁) までの相対位置 */
  line: Pt | null;
}

const overlap = (a: Box, b: Box) => a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1;
const inside = (b: Box, o: Box) => b.x0 >= o.x0 && b.y0 >= o.y0 && b.x1 <= o.x1 && b.y1 <= o.y1;
const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(v, lo), hi);

/** 点 dot の周りの置き場所 (近い順)。札の大きさは box と同じ */
export function candidates(box: Box, dot: Pt): Box[] {
  const [w, h] = [box.x1 - box.x0, box.y1 - box.y0];
  return GAPS.flatMap((gap) =>
    DIRS.map(([ux, uy]) => {
      const [cx, cy] = [dot[0] + ux * (w / 2 + gap), dot[1] + uy * (h / 2 + gap)];
      return { x0: cx - w / 2, y0: cy - h / 2, x1: cx + w / 2, y1: cy + h / 2 };
    }),
  );
}

/**
 * 札の四角 (優先の高い順) の置き場所。
 * まず元の位置、だめなら点の周りの海 (isLand でない所)、それでもだめなら陸の上の空き。
 * warned は警報以上の区域に重なる所で、札で隠さないよう blockers と同じく避ける (どこも空かなければ元の位置のまま)。
 * blockers (凡例・別枠・案内・時計) や先に置いた札と重なる所、地図の外 (bounds) にはみ出す所には置かない。どこにも置けなければ null。
 */
export function placeBadges(
  boxes: Box[],
  dots: Pt[],
  blockers: Box[],
  bounds: Box,
  isLand: (b: Box) => boolean,
  warned: (b: Box) => boolean = () => false,
): (Placement | null)[] {
  const placed: Box[] = [...blockers];
  const free = (b: Box) => !placed.some((p) => overlap(b, p));
  return boxes.map((box, i) => {
    if (free(box) && !warned(box)) {
      placed.push(box);
      return { dx: 0, dy: 0, line: null };
    }
    const dot = dots[i];
    const others = dots.filter((_, j) => j !== i).map(([x, y]): Box => ({ x0: x - DOT_R, y0: y - DOT_R, x1: x + DOT_R, y1: y + DOT_R }));
    const spots = candidates(box, dot).filter((c) => inside(c, bounds) && free(c) && !warned(c) && !others.some((o) => overlap(c, o)));
    const spot = spots.find((c) => !isLand(c)) ?? spots[0];
    if (!spot) {
      // 警報の区域だけに塞がれているときは、札を隠さず元の位置に残す (警報は見えにくいまま)
      if (!free(box)) return null;
      placed.push(box);
      return { dx: 0, dy: 0, line: null };
    }
    placed.push(spot);
    const line: Pt = [clamp(dot[0], spot.x0, spot.x1) - dot[0], clamp(dot[1], spot.y0, spot.y1) - dot[1]];
    return { dx: spot.x0 - box.x0, dy: spot.y0 - box.y0, line };
  });
}

const box = (e: Element): Box => {
  const r = e.getBoundingClientRect();
  return { x0: r.left, y0: r.top, x1: r.right, y1: r.bottom };
};

/** 箱の中心と四隅のどれかが陸 (県の塗り) の上か */
const onLand = (b: Box): boolean =>
  [[(b.x0 + b.x1) / 2, (b.y0 + b.y1) / 2], [b.x0, b.y0], [b.x1, b.y0], [b.x0, b.y1], [b.x1, b.y1]].some(([x, y]) =>
    document.elementsFromPoint(x, y).some((e) => e.classList.contains("pref")),
  );

/** 札の四角を横 5 x 縦 3 の点で調べ、警報以上 (注意報は除く) の塗りが 1 点でもあれば真。警報が無ければ呼ばない */
const onWarning = (b: Box): boolean => {
  for (let i = 0; i < 5; i++) {
    for (let j = 0; j < 3; j++) {
      const [x, y] = [b.x0 + ((b.x1 - b.x0) * i) / 4, b.y0 + ((b.y1 - b.y0) * j) / 2];
      if (document.elementsFromPoint(x, y).some((e) => e.matches(".warn[data-level]:not([data-level=advisory])"))) return true;
    }
  }
  return false;
};

/**
 * 札の層 (子は優先の高い順) の札を置き直す。
 * 動かした札は badge の transform で動かして引き出し線を出し、置けない札は thin クラスで札だけ隠す (点は残す)。
 */
export function thinCityLayer(layer: Element, blockers: Element[], bounds: Element): void {
  const cities = [...layer.children];
  for (const g of cities) {
    g.classList.remove("thin");
    g.querySelector(".city-badge")!.removeAttribute("transform");
    g.querySelector(".city-leader")!.setAttribute("visibility", "hidden");
  }
  const dots = cities.map((g): Pt => {
    const r = box(g.querySelector(".city-dot")!);
    return [(r.x0 + r.x1) / 2, (r.y0 + r.y1) / 2];
  });
  const places = placeBadges(
    cities.map((g) => box(g.querySelector(".city-box")!)),
    dots,
    blockers.map(box),
    box(bounds),
    onLand,
    // 警報以上が出ていないときは調べない (札の位置は今までと同じ)
    document.querySelector(".warn[data-level]:not([data-level=advisory])") ? onWarning : undefined,
  );
  cities.forEach((g, i) => {
    const p = places[i];
    g.classList.toggle("thin", !p);
    if (!p?.line) return;
    g.querySelector(".city-badge")!.setAttribute("transform", `translate(${p.dx} ${p.dy})`);
    const leader = g.querySelector(".city-leader")!;
    leader.setAttribute("x2", String(p.line[0]));
    leader.setAttribute("y2", String(p.line[1]));
    leader.removeAttribute("visibility");
  });
}
