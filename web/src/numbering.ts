// 直近の地震に一時的な番号を振る (地図の震央・一覧・バナーで同じ地震を見分けるため)。

export interface Numbered {
  key: string;
  /** 同じ地震の EEW のグループ (あれば同じ番号にする) */
  linkedTo?: string;
}

/** active は起きた順。前回の番号は保ち、新しいものに続きの番号を振る。active が空なら 1 から振り直す */
export function assignNumbers(prev: Map<string, number>, active: Numbered[]): Map<string, number> {
  const out = new Map<string, number>();
  let next = Math.max(0, ...active.map((a) => prev.get(a.key) ?? 0)) + 1;
  for (const a of active) {
    const n = prev.get(a.key) ?? (a.linkedTo != null ? (out.get(a.linkedTo) ?? prev.get(a.linkedTo)) : undefined) ?? next++;
    out.set(a.key, n);
  }
  return out;
}
