// 緊急地震速報の常設パネルの見せ方を決める (DOM に触らない。エスケープは描く側の仕事)
import { scaleLabel } from "./scale.ts";
import type { EewEvent } from "./types.ts";

export interface EewPanelRow {
  label: string;
  value: string;
}

export interface EewPanelView {
  /** "active" = 発表中の EEW がある / "none" = 無い */
  state: "active" | "none";
  /** 色分け用。active の 1 件目が警報なら true */
  warning: boolean;
  title: string;
  rows: EewPanelRow[];
  /** 1 件目のほかに発表中の件数 (0 なら出さない) */
  more: number;
  /** 行の代わりに出す一文 (none のとき) */
  message: string | null;
}

const JST = new Intl.DateTimeFormat("en-US", {
  timeZone: "Asia/Tokyo",
  month: "2-digit",
  day: "2-digit",
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
  hourCycle: "h23",
});

/** 日本時間の各部分 (ゼロ埋め 2 桁)。実行環境の TZ に依らない */
function jst(ms: number): Record<string, string> {
  return Object.fromEntries(JST.formatToParts(ms).map((p) => [p.type, p.value]));
}

const num = (v: number | null | undefined): v is number => typeof v === "number" && Number.isFinite(v);
const kind = (e: EewEvent) => (e.warning ? "警報" : "予報");

function lastRow(e: EewEvent): EewPanelRow {
  const t = jst(e.origin_time_ms ?? e.received_at_ms);
  const name = e.hypocenter?.name || "震源不明";
  return {
    label: "最後の発表",
    value: `${t.month}/${t.day} ${t.hour}:${t.minute} ${name} (${kind(e)}・予測最大震度${scaleLabel(e.max_scale)})`,
  };
}

/** active は優先順 (揺れの大きい順) に並んだ発表中の EEW (並べ替えは呼び出し側)。last は取り消しでない最後の EEW (無ければ null) */
export function eewPanelView(active: readonly EewEvent[], last: EewEvent | null): EewPanelView {
  const e = active[0];
  if (!e) {
    return {
      state: "none",
      warning: false,
      title: "緊急地震速報",
      rows: last ? [lastRow(last)] : [],
      more: 0,
      message: "現在、発表はありません",
    };
  }
  const h = e.hypocenter;
  const t = e.origin_time_ms == null ? null : jst(e.origin_time_ms);
  return {
    state: "active",
    warning: e.warning,
    title: `${e.test ? "【テスト】" : ""}緊急地震速報 (${kind(e)})`,
    rows: [
      { label: "震源", value: h?.name || "調査中" },
      { label: "発生", value: t ? `${t.hour}:${t.minute}:${t.second}` : "—" },
      { label: "規模", value: num(h?.magnitude) ? `M${h.magnitude.toFixed(1)}` : "—" },
      { label: "深さ", value: !num(h?.depth_km) ? "—" : h.depth_km === 0 ? "ごく浅い" : `約${Math.round(h.depth_km)}km` },
      { label: "予測最大震度", value: scaleLabel(e.max_scale) },
      { label: "報", value: `第${e.serial}報` },
    ],
    more: active.length - 1,
    message: null,
  };
}
