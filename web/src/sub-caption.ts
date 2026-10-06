// サブの地図の見出しの札と凡例の中身。詳細パネル (view.ts) と同じ文言をここから使う。DOM には触らない。

import { eewAreaScales } from "./detail.ts";
import { type Group, heldEew, summarizeQuake } from "./groups.ts";
import type { EewEvent, Hypocenter, Scale } from "./types.ts";

export const TSUNAMI_TEXT: Record<string, string> = {
  None: "この地震による津波の心配はありません",
  NonEffective: "若干の海面変動 (被害の心配なし)",
  Checking: "津波の有無を調査中",
  Watch: "津波注意報 発表中",
  Warning: "津波警報等 発表中",
};

/** 震源の規模と深さ ("M5.3 / 深さ40km")。無ければ空文字 */
export function hypoFacts(h: Hypocenter | null): string {
  if (!h) return "";
  const parts: string[] = [];
  if (h.magnitude != null) parts.push(`M${h.magnitude.toFixed(1)}`);
  if (h.depth_km === 0) parts.push("ごく浅い");
  else if (h.depth_km != null) parts.push(`深さ${h.depth_km}km`);
  return parts.join(" / ");
}

/** 震源の一行 (名前 / M / 深さ)。エスケープはしない (HTML にするときは呼ぶ側で esc) */
export function hypoText(h: Hypocenter | null): string {
  if (!h) return "震源調査中";
  return [h.name || "震源不明", hypoFacts(h)].filter(Boolean).join(" / ");
}

/** 緊急地震速報の見出し "緊急地震速報 (予報) [テスト] 第5報" */
export function eewKindLabel(e: EewEvent): string {
  return `緊急地震速報 (${e.warning ? "警報" : "予報"})${e.test ? " [テスト]" : ""} 第${e.serial}報`;
}

export interface SubCaption {
  /** 最大震度 (EEW は予想最大震度)。分からなければ -1 */
  scale: Scale;
  /** 例 "緊急地震速報 (予報) 第5報" / "震度速報" / "各地の震度に関する情報" (INFO_LABELS) */
  kind: string;
  /** EEW の予報/警報の色分け用 */
  eew: "forecast" | "warning" | null;
  /** 震源名。無ければ "震源調査中" (EEW は "震源不明")、取り消しは "取り消されました" */
  title: string;
  /** "M5.3 / 深さ40km" など (hypoText と同じ規則。無ければ空文字) */
  facts: string;
  /** 発生時刻の文字列 (詳細パネルと同じもの) + " 発生" */
  time: string;
  /** 津波の一文 (TSUNAMI_TEXT)。EEW は null */
  tsunami: string | null;
}

export interface SubLegend {
  /** 出ている震度の段階 (大きい順、重複なし)。地震情報は観測点と県の最大の震度、EEW は予想の区域・県の震度 */
  scales: Scale[];
  /** EEW の予想の塗りを描いている */
  forecast: boolean;
  /** 震央を描いている */
  epicenter: boolean;
  /** P 波・S 波の円を描いている */
  waves: boolean;
}

export function subCaption(g: Group): SubCaption | null {
  if (g.kind === "quake") {
    const q = summarizeQuake(g);
    return {
      scale: q.maxScale,
      kind: q.infoLabel,
      eew: null,
      title: q.hypocenter?.name || "震源調査中",
      facts: hypoFacts(q.hypocenter),
      time: `${q.originTime} 発生`,
      tsunami: TSUNAMI_TEXT[q.domesticTsunami] ?? "—",
    };
  }
  if (g.kind === "eew") {
    const e = heldEew(g);
    return {
      scale: e.max_scale,
      kind: eewKindLabel(e),
      eew: e.warning ? "warning" : "forecast",
      title: e.cancelled ? "取り消されました" : (e.hypocenter?.name ?? "震源不明"),
      facts: hypoFacts(e.hypocenter),
      time: `${e.origin_time ?? e.issued_at} 発生`,
      tsunami: null,
    };
  }
  return null;
}

const hasPosition = (h: Hypocenter | null): boolean => h?.latitude != null && h.longitude != null;

/** 大きい順・重複なし・不明 (0 以下) は除く */
const descending = (xs: Scale[]): Scale[] => [...new Set(xs.filter((s) => s > 0))].sort((a, b) => b - a);

export function subLegend(g: Group, waving: boolean): SubLegend | null {
  if (g.kind === "quake") {
    const q = summarizeQuake(g);
    return {
      scales: descending([...q.points.map((p) => p.scale), ...q.prefMax.map((p) => p.scale)]),
      forecast: false,
      epicenter: hasPosition(q.hypocenter),
      waves: waving,
    };
  }
  if (g.kind === "eew") {
    const e = heldEew(g);
    if (e.cancelled) return { scales: [], forecast: false, epicenter: false, waves: false };
    return {
      scales: descending([...eewAreaScales(e.areas).map((a) => a.scale), ...e.pref_max.map((p) => p.scale)]),
      forecast: e.areas.length > 0 || e.pref_max.length > 0,
      epicenter: hasPosition(e.hypocenter),
      waves: waving,
    };
  }
  return null;
}
