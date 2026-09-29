import type { Scale } from "./types.ts";

// 気象庁の震度配色に準拠
const COLORS: Record<number, string> = {
  10: "#f2f2ff",
  20: "#00aaff",
  30: "#0041ff",
  40: "#faf500",
  45: "#ffe600",
  46: "#ffe600",
  47: "#ffe600",
  50: "#ff9900",
  55: "#ff2800",
  57: "#ff2800",
  60: "#a50021",
  70: "#b40068",
};

const LABELS: Record<number, string> = {
  10: "1",
  20: "2",
  30: "3",
  40: "4",
  45: "5弱",
  46: "5弱以上",
  // 1996 年 3 月までの震度階級の 5 と 6 (弱・強の区別が無い)。過去の地震の記録のデモでだけ使う。色は 5弱・6弱 に合わせる
  47: "5",
  50: "5強",
  55: "6弱",
  57: "6",
  60: "6強",
  70: "7",
};

export function scaleColor(s: Scale): string {
  return COLORS[s] ?? "#666a73";
}

/** 背景色に対して読みやすい文字色 */
export function scaleTextColor(s: Scale): string {
  return s === 10 || s === 40 || s === 45 || s === 46 || s === 47 ? "#111" : "#fff";
}

export function scaleLabel(s: Scale): string {
  return LABELS[s] ?? "不明";
}
