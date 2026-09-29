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
  /** 市町村等のコード -> 名前 */
  names?: Record<string, string>;
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

/** 都道府県 (市町村等のコードの頭 2 桁が 01〜47 の番号) */
const PREFS = (
  "北海道 青森県 岩手県 宮城県 秋田県 山形県 福島県 茨城県 栃木県 群馬県 埼玉県 千葉県 東京都 神奈川県 " +
  "新潟県 富山県 石川県 福井県 山梨県 長野県 岐阜県 静岡県 愛知県 三重県 滋賀県 京都府 大阪府 兵庫県 " +
  "奈良県 和歌山県 鳥取県 島根県 岡山県 広島県 山口県 徳島県 香川県 愛媛県 高知県 福岡県 佐賀県 長崎県 " +
  "熊本県 大分県 宮崎県 鹿児島県 沖縄県"
).split(" ");

export function prefOf(code: string): string {
  return PREFS[Number(code.slice(0, 2)) - 1] ?? "";
}

/**
 * 警報以上 (警報・危険警報・特別警報) を種類ごとの文にする。注意報は含めない。
 * 「レベル３大雨警報: 岡山県 岡山市・倉敷市、広島県 福山市」。1 県 perPref 市町村までで、残りは「ほかN」
 */
export function warningSummary(w: Warnings & { names?: Record<string, string> }, perPref = 5): { top: WarningLevel; lines: string[] } | null {
  const byKind = new Map<string, { level: WarningLevel; prefs: Map<string, string[]> }>();
  for (const code of Object.keys(w.areas).sort()) {
    for (const k of w.areas[code]) {
      const level = warningLevel(k.name);
      if (level === "advisory") continue;
      const entry = byKind.get(k.name) ?? { level, prefs: new Map<string, string[]>() };
      byKind.set(k.name, entry);
      const pref = prefOf(code);
      const names = entry.prefs.get(pref) ?? [];
      entry.prefs.set(pref, [...names, w.names?.[code] ?? code]);
    }
  }
  if (byKind.size === 0) return null;
  const kinds = [...byKind].sort(([a, x], [b, y]) => ORDER.indexOf(y.level) - ORDER.indexOf(x.level) || a.localeCompare(b));
  const lines = kinds.map(([name, { prefs }]) => {
    const parts = [...prefs].map(([pref, ms]) => `${pref} ${ms.slice(0, perPref).join("・")}${ms.length > perPref ? ` ほか${ms.length - perPref}` : ""}`);
    return `${name}: ${parts.join("、")}`;
  });
  return { top: kinds[0][1].level, lines };
}
