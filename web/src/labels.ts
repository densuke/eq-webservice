// 塗り分けた地域に出す震度の数字: 地域の内側に置く点と、重ならないように残すものの選び方。

type Pt = [number, number];

function ringArea(r: Pt[]): number {
  let a = 0;
  for (let i = 0, j = r.length - 1; i < r.length; j = i++) a += (r[j][0] + r[i][0]) * (r[j][1] - r[i][1]);
  return Math.abs(a / 2);
}

function inside(p: Pt, r: Pt[]): boolean {
  let c = false;
  for (let i = 0, j = r.length - 1; i < r.length; j = i++) {
    const [xi, yi] = r[i];
    const [xj, yj] = r[j];
    if (yi > p[1] !== yj > p[1] && p[0] < ((xj - xi) * (p[1] - yi)) / (yj - yi) + xi) c = !c;
  }
  return c;
}

function distToEdges(p: Pt, r: Pt[]): number {
  let best = Infinity;
  for (let i = 0, j = r.length - 1; i < r.length; j = i++) {
    const [ax, ay] = r[j];
    const [bx, by] = r[i];
    const dx = bx - ax;
    const dy = by - ay;
    const t = dx || dy ? Math.max(0, Math.min(1, ((p[0] - ax) * dx + (p[1] - ay) * dy) / (dx * dx + dy * dy))) : 0;
    best = Math.min(best, Math.hypot(p[0] - ax - t * dx, p[1] - ay - t * dy));
  }
  return best;
}

/**
 * 地域の内側で、縁から最も離れた点 (の近似)。一番大きい輪を格子で調べる。
 * 外接矩形の中心は、湾や入り組んだ形では外 (海) に落ちるため使わない
 */
/** いちばん大きい輪 (離島を除いた本土) */
export function mainRing(rings: Pt[][]): Pt[] {
  return rings.reduce((a, b) => (ringArea(b) > ringArea(a) ? b : a));
}

export function interiorPoint(rings: Pt[][], grid = 12): Pt {
  const r = mainRing(rings);
  const xs = r.map((p) => p[0]);
  const ys = r.map((p) => p[1]);
  const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
  let best: Pt = r[0];
  let bestD = -1;
  for (let i = 0; i <= grid; i++) {
    for (let j = 0; j <= grid; j++) {
      const p: Pt = [x0 + ((x1 - x0) * (i + 0.5)) / (grid + 1), y0 + ((y1 - y0) * (j + 0.5)) / (grid + 1)];
      if (!inside(p, r)) continue;
      const d = distToEdges(p, r);
      if (d > bestD) [best, bestD] = [p, d];
    }
  }
  return best;
}

interface LabelBox {
  key: string;
  /** 中心と大きさ (どれも同じ座標系) */
  x: number;
  y: number;
  w: number;
  h: number;
  scale: number;
}

/** 重なるものは震度の大きい方を残す */
export function pickLabels<T extends LabelBox>(items: T[]): T[] {
  const out: T[] = [];
  for (const l of [...items].sort((a, b) => b.scale - a.scale)) {
    const hit = out.some((o) => Math.abs(o.x - l.x) * 2 < o.w + l.w && Math.abs(o.y - l.y) * 2 < o.h + l.h);
    if (!hit) out.push(l);
  }
  return out;
}

/** 文字の大きさ (px)。震度1 13px 〜 震度7 24px */
export function labelPx(scale: number): number {
  const s = Math.min(Math.max(scale, 10), 70);
  return Math.round(13 + ((s - 10) / 60) * 11);
}
