// 右上のサブの地図 (最近の地震): 表示中の地震へ寄って塗る。保持時間のあいだはそのまま、過ぎたら薄く残す。

import { pad, pointBox, union } from "./camera.ts";
import { $, subMap } from "./dom.ts";
import { subMapState } from "./hold.ts";
import { subMapConfig } from "./layout-dom.ts";
import { project } from "./map.ts";
import { currentGroup, geoOf, groupScale, relatedQuake, shakenGeo } from "./quakes.ts";
import { paintMap } from "./view.ts";

const box = $("#map-sub");
let lastSig: string | null = null;

/** 毎 tick 呼ぶ。表示する地震・保持の状態・枠の大きさが前回と同じなら何もしない */
export function renderSubMap(now: number): void {
  const w = box.clientWidth;
  const h = box.clientHeight;
  // 隠し置き場 (trial 以外) にいる間は描かない
  if (w === 0 || h === 0) return;
  const g = currentGroup();
  const q = g && relatedQuake(g);
  const cfg = subMapConfig();
  const state = subMapState(now, q, q ? groupScale(q) : -1, cfg);
  const sig = `${state && q ? `${state.key}|${q.updatedAt}|${state.faded}` : ""}|${w}x${h}`;
  if (sig === lastSig) return;
  lastSig = sig;

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
  subMap.setFade(state.faded ? cfg.fadedAlpha : 1);
  const shaken = shakenGeo(q);
  const shakenBox = subMap.areaBox(shaken.areas) ?? subMap.prefBox(shaken.prefs);
  const [x, y] = c ? project(c.lon, c.lat) : [0, 0];
  const view = union(shakenBox, c ? pointBox(x, y, 80) : null);
  subMap.jumpTo(view && pad(view));
}
