// 気象警報・注意報 (平時の地図)。気象庁防災情報XML の集約通報をサーバがまとめたもの。

export interface WarningKind {
  code: string;
  name: string;
}

/** GET /api/warnings */
export interface Warnings {
  reported_at: string;
  /** 市町村等のコード -> 発表中の警報・注意報 */
  areas: Record<string, WarningKind[]>;
}

/** 段階 (色分け)。レベル2 注意報 = 黄、レベル3 警報 = 赤、レベル4 危険警報 = 紫、レベル5 特別警報 = 黒 */
export type WarningLevel = "advisory" | "warning" | "danger" | "emergency";
const ORDER: WarningLevel[] = ["advisory", "warning", "danger", "emergency"];

export function warningLevel(name: string): WarningLevel {
  if (name.includes("特別警報")) return "emergency";
  if (name.includes("危険警報")) return "danger";
  if (name.includes("警報")) return "warning";
  return "advisory";
}

/** その区域で一番高い段階 */
export function topLevel(kinds: WarningKind[]): WarningLevel {
  return kinds.map((k) => warningLevel(k.name)).reduce((a, b) => (ORDER.indexOf(b) > ORDER.indexOf(a) ? b : a), "advisory");
}
