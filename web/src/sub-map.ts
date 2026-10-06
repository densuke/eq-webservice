// 右上のサブの地図 (最近の地震): 表示中の地震へ寄って塗る。保持時間のあいだはそのまま、過ぎたら薄く残す。

import { pad, pointBox, union } from "./camera.ts";
import { $, subMap } from "./dom.ts";
import { app } from "./state.ts";
import type { Group } from "./groups.ts";
import { type SubMapState, subMapSigs, subMapState } from "./hold.ts";
import { esc } from "./html.ts";
import { subMapConfig } from "./layout-dom.ts";
import { project } from "./map.ts";
import { currentGroup, geoOf, groupPlace, groupScale, relatedQuake, shakenGeo } from "./quakes.ts";
import { sameQuake } from "./priority.ts";
import type { MainWave } from "./scene.ts";
import { scaleColor, scaleLabel } from "./scale.ts";
import { subCaption, subLegend } from "./sub-caption.ts";
import { badge, paintMap } from "./view.ts";

const box = $("#map-sub");
let lastPaint: string | null = null;
let lastFade: string | null = null;
/** サブの地図に映している地震 (描き直したときの) */
let shownQuake: Group | undefined;
/** サブの地図に P 波・S 波を描いているか */
let waving = false;
/** 保持時間を過ぎて薄くなっている間か (波の線は薄くならないので、その間は波を描かない) */
let faded = false;
/** 凡例にP波・S波以外の行があるか (波の行だけ出し入れするとき、凡例ごと隠すかの判断に使う) */
let legendHasRows = false;

// 見出しの札 (左上) と凡例 (右下)。中身は paint() で作る。同じ内容が右列の詳細パネルにあるので読み上げからは外す
const caption = overlay("sub-caption");
const legend = overlay("sub-legend");
const waveRow = document.createElement("div");
waveRow.className = "sub-legend-row";
waveRow.innerHTML = `<span class="lw p">P波</span> <span class="lw s">S波</span>`;

function overlay(cls: string): HTMLElement {
  const e = document.createElement("div");
  e.className = cls;
  e.hidden = true;
  e.setAttribute("aria-hidden", "true");
  box.append(e);
  return e;
}

/** 枠が見えているか (trial 以外では隠し置き場で 0 のまま) */
export function subMapShown(): boolean {
  return box.clientWidth > 0 && box.clientHeight > 0;
}

/** 次の renderSubMap で必ず描き直す (表示中の地震と同じ地震の別のグループが更新されたときなど) */
export function invalidateSubMap(): void {
  lastPaint = null;
}

/** 毎 tick 呼ぶ。表示する地震・枠の大きさが変わったら描き直し、薄さだけが変わったら setFade だけ呼ぶ */
export function renderSubMap(now: number): void {
  // 隠し置き場 (trial 以外) にいる間は描かない。再び見えたときにカメラを合わせ直すため署名を消す
  if (!subMapShown()) {
    invalidateSubMap();
    // 定義を切り替えたあとに古い波の状態が残らないよう、波の状態も消す
    shownQuake = undefined;
    waving = false;
    faded = false;
    return;
  }
  const w = box.clientWidth;
  const h = box.clientHeight;
  const g = currentGroup();
  const q = g && relatedQuake(g);
  const cfg = subMapConfig();
  const state = subMapState(now, q, q ? groupScale(q) : -1, cfg);
  faded = !!state && state.alpha < 1;
  const sigs = subMapSigs(state, q, w, h);
  if (sigs.paint !== lastPaint) {
    lastPaint = sigs.paint;
    lastFade = sigs.fade;
    paint(state, q);
  } else if (sigs.fade !== lastFade) {
    lastFade = sigs.fade;
    setFade(state?.alpha ?? 1);
  }
}

function setFade(alpha: number): void {
  subMap.setFade(alpha);
  caption.style.opacity = legend.style.opacity = String(alpha);
}

/** 主の地図の波 (毎 tick)。サブの地図に映している地震の波だけ描く。塗りは描き直さない */
export function renderSubWaves(wave: MainWave | null): void {
  if (!subMapShown()) return;
  const g = wave && shownQuake && (wave.key === shownQuake.key ? shownQuake : app.world.store.get(wave.key));
  const mine = !faded && !!(g && wave && (g === shownQuake || sameQuake(groupPlace(g), groupPlace(shownQuake!))));
  subMap.setWaves(mine && wave ? [wave] : []);
  if (mine === waving) return;
  waving = mine;
  waveRow.hidden = !mine;
  legend.hidden = !legendHasRows && !mine;
}

/** 札の中身。epicenter の印は ✕ */
function captionHtml(q: Group): string {
  const c = subCaption(q);
  if (!c) return "";
  const kindCls = c.eew === "warning" ? " warning" : c.eew === "forecast" ? " forecast" : "";
  return `${badge(c.scale, true)}<div class="sub-caption-text"><div class="sub-kind${kindCls}">${esc(c.kind)}</div><div class="sub-title">${esc(c.title)}</div>${
    c.facts ? `<div class="sub-facts">${esc(c.facts)}</div>` : ""
  }<div class="sub-time">${esc(c.time)}</div>${c.tsunami ? `<div class="sub-tsunami">${esc(c.tsunami)}</div>` : ""}</div>`;
}

/** 凡例の行 (震度・予想・震央)。P波・S波の行は波が出入りするので waveRow で別に扱い、subLegend の waves は使わない (false を渡す) */
function legendRows(q: Group): string[] {
  const l = subLegend(q, false);
  if (!l) return [];
  const rows = l.scales.map((s) => `<div class="sub-legend-row"><i style="background:${scaleColor(s)}"></i>震度${esc(scaleLabel(s))}</div>`);
  if (l.forecast) rows.push(`<div class="sub-legend-row"><i class="forecast"></i>予想</div>`);
  if (l.epicenter) rows.push(`<div class="sub-legend-row"><span class="x">✕</span>震央</div>`);
  return rows;
}

function hideOverlays(): void {
  caption.hidden = true;
  legend.hidden = true;
  caption.replaceChildren();
  legend.replaceChildren();
  shownQuake = undefined;
  legendHasRows = false;
}

function paint(state: SubMapState | null, q: Group | undefined): void {
  if (!state || !q) {
    paintMap(subMap, undefined);
    subMap.setEpicenters([]);
    subMap.setFade(1);
    subMap.jumpTo(null);
    hideOverlays();
    return;
  }
  shownQuake = q;
  const rows = legendRows(q);
  legendHasRows = rows.length > 0;
  legend.innerHTML = rows.join("");
  legend.append(waveRow);
  waveRow.hidden = !waving;
  legend.hidden = !legendHasRows && !waving;
  caption.innerHTML = captionHtml(q);
  caption.hidden = false;
  paintMap(subMap, q);
  const c = geoOf(q)?.center;
  subMap.setEpicenters(c ? [{ key: q.key, lat: c.lat, lon: c.lon, label: null, primary: true, scale: groupScale(q) }] : []);
  setFade(state.alpha);
  const shaken = shakenGeo(q);
  const shakenBox = subMap.areaBox(shaken.areas) ?? subMap.prefBox(shaken.prefs);
  const [x, y] = c ? project(c.lon, c.lat) : [0, 0];
  const view = union(shakenBox, c ? pointBox(x, y, 80) : null);
  const padded = view && pad(view);
  // 札が左上を覆うので、寄る範囲を札の高さの分だけ上へ広げる (札の高さは描いたあとの実測)
  const capH = caption.offsetHeight;
  const h = box.clientHeight;
  if (padded && capH > 0 && h > capH) padded.y0 -= ((padded.y1 - padded.y0) * capH) / (h - capH);
  subMap.jumpTo(padded);
}
