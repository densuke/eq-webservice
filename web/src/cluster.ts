// 画面上で重なる震央の印を 1 つにまとめる (番号は時系列順に並べ、揺れの大きい地震ほど大きく)。

export interface Marker {
  x: number;
  y: number;
  /** 一時的な番号 (起きた順) */
  label: number;
  scale: number;
  primary: boolean;
}

export interface Cluster {
  x: number;
  y: number;
  primary: boolean;
  labels: { label: number; scale: number }[];
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
    labels: members.map((m) => ({ label: m.label, scale: m.scale })).sort((a, b) => a.label - b.label),
  }));
}

/** 番号の文字の大きさ (px)。震度1 以下 12px 〜 震度7 20px */
export function labelSize(scale: number): number {
  const s = Math.min(Math.max(scale, 10), 70);
  return Math.round(12 + ((s - 10) / 60) * 8);
}
