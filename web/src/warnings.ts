// 気象警報・注意報 (平時の地図)。気象庁防災情報XML の集約通報をサーバがまとめたもの。

import type { Box } from "./camera.ts";

interface WarningKind {
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
type WarningLevel = "advisory" | "warning" | "danger" | "emergency";
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

/** 警報以上の 1 種類: 「レベル３大雨警報」と「岡山県 岡山市・倉敷市、広島県 福山市」 */
export interface WarnItem {
  level: WarningLevel;
  name: string;
  places: string;
}

/** 警報以上 (警報・危険警報・特別警報) を種類ごとにまとめる。注意報は含めない。段階の高い順。1 県 perPref 市町村までで、残りは「ほかN」 */
export function warningItems(w: Warnings, perPref = 5): WarnItem[] {
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
  return [...byKind]
    .sort(([a, x], [b, y]) => ORDER.indexOf(y.level) - ORDER.indexOf(x.level) || a.localeCompare(b))
    .map(([name, { level, prefs }]) => ({
      level,
      name,
      places: [...prefs].map(([pref, ms]) => `${pref} ${ms.slice(0, perPref).join("・")}${ms.length > perPref ? ` ほか${ms.length - perPref}` : ""}`).join("、"),
    }));
}

/** 警報以上を種類ごとの文にする。「レベル３大雨警報: 岡山県 岡山市・倉敷市、広島県 福山市」 */
export function warningSummary(w: Warnings, perPref = 5): { top: WarningLevel; lines: string[] } | null {
  const items = warningItems(w, perPref);
  return items.length ? { top: items[0].level, lines: items.map((i) => `${i.name}: ${i.places}`) } : null;
}

/** 平時に地図を寄せて見せる、危険警報以上の区域 (1 県、または隣り合う数県)。items (札、市町村は全部) と lines (案内、1 県 3 市町村まで) はその県の危険警報以上だけ */
export interface WarnFocus {
  prefs: string[];
  codes: string[];
  items: WarnItem[];
  lines: string[];
  top: WarningLevel;
}

const FOCUSED: WarningLevel[] = ["danger", "emergency"];
/** この数の県に危険警報以上が出ていたら寄せない (全国の地図のほうが分かり、巡回も長くなりすぎる) */
const MAX_FOCUS_PREFS = 6;
function focusOf(prefs: string[], areas: Record<string, WarningKind[]>, names: Warnings["names"]): WarnFocus {
  const w = { reported_at: "", areas, names };
  const items = warningItems(w, Infinity);
  return { prefs, codes: Object.keys(areas).sort(), items, lines: warningSummary(w, 3)!.lines, top: items[0].level };
}

/** 危険警報以上の区域を県ごとにまとめる。特別警報の県があればその県だけ。段階の高い県から、同じなら県の番号順 */
export function warnFocuses(w: Warnings | null): WarnFocus[] {
  if (!w) return [];
  const byPref = new Map<string, Record<string, WarningKind[]>>();
  for (const code of Object.keys(w.areas).sort()) {
    const kinds = w.areas[code].filter((k) => FOCUSED.includes(warningLevel(k.name)));
    if (kinds.length === 0) continue;
    const pref = prefOf(code);
    byPref.set(pref, { ...byPref.get(pref), [code]: kinds });
  }
  const all = [...byPref].map(([pref, areas]) => focusOf([pref], areas, w.names));
  const emergency = all.filter((f) => f.top === "emergency");
  const fs = emergency.length ? emergency : all;
  return fs.length >= MAX_FOCUS_PREFS ? [] : fs;
}

/**
 * 1 画面に収まる (外接矩形の幅・高さが maxUnits 以下) 県どうしを、段階の高い順にまとめる。
 * boxOf は区域のコードの外接矩形 (地図座標)。区域が読めていなければ null で、まとめない
 */
export function mergeFocuses(fs: WarnFocus[], w: Warnings, boxOf: (codes: string[]) => Box | null, maxUnits: number): WarnFocus[] {
  const fits = (codes: string[]) => {
    const b = boxOf(codes);
    return b != null && b.x1 - b.x0 <= maxUnits && b.y1 - b.y0 <= maxUnits;
  };
  const groups = fs.reduce<WarnFocus[][]>((gs, f) => {
    const i = gs.findIndex((g) => fits([...g.flatMap((x) => x.codes), ...f.codes]));
    return i < 0 ? [...gs, [f]] : gs.map((g, j) => (j === i ? [...g, f] : g));
  }, []);
  return groups.map((g) =>
    g.length === 1
      ? g[0]
      : focusOf(
          g.flatMap((f) => f.prefs),
          Object.fromEntries(g.flatMap((f) => f.codes).map((c) => [c, w.areas[c].filter((k) => FOCUSED.includes(warningLevel(k.name)))])),
          w.names,
        ),
  );
}

/** 1 分のうち地図を寄せる秒数 (毎分の後ろ)。寄せる先が複数なら 1 分ごとに次へ */
const FOCUS_SEC = 15;

export function warnFocusAt(fs: WarnFocus[], now: number): WarnFocus | null {
  if (fs.length === 0 || (now / 1000) % 60 < 60 - FOCUS_SEC) return null;
  return fs[Math.floor(now / 60_000) % fs.length];
}
