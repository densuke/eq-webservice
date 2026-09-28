// 画面上で重なる震央の印を 1 つにまとめる (番号は時系列順に並べ、揺れの大きい地震ほど大きく)。

export interface Marker {
  x: number;
  y: number;
  /** 一時的な番号 (起きた順) */
  label: number;
  scale: number;
  primary: boolean;
  /** 地震のグループのキー (印を押したときに選ぶ) */
  key?: string;
}

export interface Cluster {
  x: number;
  y: number;
  primary: boolean;
  labels: { label: number; scale: number; key?: string }[];
}

/** dist (地図座標) より近い印をまとめる。先に置いた印を基準に、近いものを吸収していく */
export function clusterMarkers(markers: Marker[], dist: number): Cluster[] {
  const out: { anchor: Marker; members: Marker[] }[] = [];
  // 表示中の地震を基準にしたいので先に置く
  for (const mk of [...markers].sort((a, b) => Number(b.primary) - Number(a.primary))) {
    const near = out.find((c) => Math.hypot(c.anchor.x - mk.x, c.anchor.y - mk.y) < dist);
    if (near) near.members.push(mk);
    else out.push({ anchor: mk, members: [mk] });
  }
  return out.map(({ anchor, members }) => ({
    x: anchor.x,
    y: anchor.y,
    primary: members.some((m) => m.primary),
    labels: members.map((m) => ({ label: m.label, scale: m.scale, key: m.key })).sort((a, b) => a.label - b.label),
  }));
}

/** 番号の文字の大きさ (px)。震度1 以下 12px 〜 震度7 20px */
export function labelSize(scale: number): number {
  const s = Math.min(Math.max(scale, 10), 70);
  return Math.round(12 + ((s - 10) / 60) * 8);
}

/**
 * 画面 (w x h px) の外にある点 (px, py) を、画面の中心からその点へ向かう線が
 * 余白 margin の内側の枠と交わる位置に置く。画面内なら null。angle は向き (度、右が 0、下が 90)
 */
export function edgePoint(px: number, py: number, w: number, h: number, margin: number): { x: number; y: number; angle: number } | null {
  if (px >= 0 && px <= w && py >= 0 && py <= h) return null;
  const cx = w / 2;
  const cy = h / 2;
  const dx = px - cx;
  const dy = py - cy;
  const t = Math.min(dx === 0 ? Infinity : (cx - margin) / Math.abs(dx), dy === 0 ? Infinity : (cy - margin) / Math.abs(dy));
  return { x: cx + dx * t, y: cy + dy * t, angle: (Math.atan2(dy, dx) * 180) / Math.PI };
}
