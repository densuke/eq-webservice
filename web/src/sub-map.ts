// 右上のサブの地図 (最近の地震): 表示中の地震へ寄って塗る。保持時間のあいだはそのまま、過ぎたら薄く残す。

import { pad, pointBox, union } from "./camera.ts";
import { $, subMap } from "./dom.ts";
import type { Group } from "./groups.ts";
import { type SubMapState, subMapSigs, subMapState } from "./hold.ts";
import { subMapConfig } from "./layout-dom.ts";
import { project } from "./map.ts";
import { currentGroup, geoOf, groupScale, relatedQuake, shakenGeo } from "./quakes.ts";
import { paintMap } from "./view.ts";

const box = $("#map-sub");
let lastPaint: string | null = null;
let lastFade: string | null = null;

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
    return;
  }
  const w = box.clientWidth;
  const h = box.clientHeight;
  const g = currentGroup();
  const q = g && relatedQuake(g);
  const cfg = subMapConfig();
  const state = subMapState(now, q, q ? groupScale(q) : -1, cfg);
  const sigs = subMapSigs(state, q, w, h);
  if (sigs.paint !== lastPaint) {
    lastPaint = sigs.paint;
    lastFade = sigs.fade;
    paint(state, q);
  } else if (sigs.fade !== lastFade) {
    lastFade = sigs.fade;
    subMap.setFade(state?.alpha ?? 1);
  }
}

function paint(state: SubMapState | null, q: Group | undefined): void {
  if (!state || !q) {
    paintMap(subMap, undefined);
    subMap.setEpicenters([]);
    subMap.setFade(1);
    subMap.jumpTo(null);
    return;
  }
  paintMap(subMap, q);
  const c = geoOf(q)?.center;
  subMap.setEpicenters(c ? [{ key: q.key, lat: c.lat, lon: c.lon, label: null, primary: true, scale: groupScale(q) }] : []);
  subMap.setFade(state.alpha);
  const shaken = shakenGeo(q);
  const shakenBox = subMap.areaBox(shaken.areas) ?? subMap.prefBox(shaken.prefs);
  const [x, y] = c ? project(c.lon, c.lat) : [0, 0];
  const view = union(shakenBox, c ? pointBox(x, y, 80) : null);
  subMap.jumpTo(view && pad(view));
}
