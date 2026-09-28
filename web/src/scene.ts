// 地図の場面: 自動カメラの目標、P波・S波の円、震央の印。

import { type Box, followRadiusKm, pad, pointBox, stopRadiusKm, union } from "./camera.ts";
import { project } from "./map.ts";
import { type Center, type WaveSource, currentGroup, geoOf, groupScale, priorityGroups, recentQuakes, relatedQuake, waveSources } from "./quakes.ts";
import { $, REPLAY_SPEED, WAVE_MAX_SEC, app, map, now } from "./state.ts";
import { activeAreas } from "./tsunami.ts";
import { VP_KM_S, VS_KM_S, surfaceRadiusKm } from "./waves.ts";

/** 震央の印。番号ごとに 1 つ (同じ地震の EEW と地震情報は地震情報の震源を使う) */
export function renderMarkers(now: number): void {
  const cur = currentGroup();
  const shown = cur && relatedQuake(cur);
  // 履歴や矢印で別の地震を選んでいる間も、直近のほかの地震の印と矢印は出す (元の地震へ戻れるように)
  const groups = [...recentQuakes(now), ...(shown ? [shown] : [])];
  const byNum = new Map<string, { key: string; lat: number; lon: number; label: number | null; primary: boolean; scale: number; quake: boolean }>();
  for (const g of groups) {
    const c = geoOf(g)?.center;
    if (!c) continue;
    const label = app.numbers.get(g.key) ?? null;
    const id = label == null ? g.key : String(label);
    const primary = g === shown || (label != null && shown != null && app.numbers.get(shown.key) === label);
    const prev = byNum.get(id);
    if (prev && (prev.quake || g.kind !== "quake")) {
      prev.primary ||= primary;
      continue;
    }
    byNum.set(id, { key: g.key, lat: c.lat, lon: c.lon, label, primary: primary || (prev?.primary ?? false), scale: groupScale(g), quake: g.kind === "quake" });
  }
  map.setEpicenters([...byNum.values()].map(({ quake: _, ...m }) => m));
}

export interface Scene {
  center: Center | null;
  /** カメラの対象以外で波を描く地震 (ライブで複数の地震が重なったとき) */
  others: WaveSource[];
  /** 発生からの秒数。null なら波は描かない */
  t: number | null;
  shaken: Box | null;
  replay: boolean;
}

/** いま地図で見せる地震。null なら日本全体 */
export function scene(now: number): Scene | null {
  if (app.selectedKey) {
    const g = app.world.store.get(app.selectedKey);
    const geo = g && relatedQuake(g) && geoOf(relatedQuake(g)!);
    if (!geo) return null;
    return { center: geo.center, others: [], t: ((now - app.selectedAt) / 1000) * REPLAY_SPEED, shaken: map.prefBox(geo.prefs), replay: true };
  }
  // カメラは揺れの大きい方に合わせる
  const [src, ...others] = waveSources(now);
  // 波が終わったら、津波予報が出ていれば予報区全体を見せる
  const tsunamiBox = map.tsunamiBox(activeAreas(app.world.tsunami).map((a) => a.name));
  if (!src && tsunamiBox) return { center: null, others: [], t: null, shaken: tsunamiBox, replay: false };
  const g = src?.group ?? priorityGroups(now)[0];
  if (!g) return null;
  const geo = geoOf(g);
  return {
    center: src ?? geo?.center ?? null,
    others,
    t: src ? (now - src.origin) / 1000 : null,
    shaken: geo ? map.prefBox(geo.prefs) : null,
    replay: false,
  };
}

/** 波を描き、カメラの目標を返す。波を描いているかどうかも返す */
export function renderScene(sc: Scene | null): { box: Box | null; waving: boolean } {
  if (!sc?.center) return { box: sc?.shaken ? pad(sc.shaken) : null, waving: false };
  const c = sc.center;
  const [x, y] = project(c.lon, c.lat);
  const stop = stopRadiusKm(x, y, sc.shaken);
  const s = sc.t == null ? null : surfaceRadiusKm(VS_KM_S, c.depth, sc.t);
  // 再生は揺れた地域を覆い終えたら打ち切る (早回しでも 180 秒分は長い)
  const waving = sc.t != null && sc.t < WAVE_MAX_SEC && !(sc.replay && (s ?? 0) > stop);
  if (!waving) return { box: pad(union(sc.shaken, pointBox(x, y, 0))!), waving };
  const wave = (w: Center, t: number) => ({ ...w, pKm: surfaceRadiusKm(VP_KM_S, w.depth, t), sKm: surfaceRadiusKm(VS_KM_S, w.depth, t) });
  map.setWaves([wave(c, sc.t!), ...sc.others.map((o) => wave(o, (now() - o.origin) / 1000))]);
  $("#wave-info").textContent = sc.replay ? `再生中 ${sc.t!.toFixed(0)}秒 (×${REPLAY_SPEED})` : `発生から${sc.t!.toFixed(0)}秒`;
  return { box: pad(pointBox(x, y, followRadiusKm(s, stop))), waving };
}
