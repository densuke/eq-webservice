// 地図の場面: 自動カメラの目標、P波・S波の円、震央の印。

import { type Box, DEFAULT_STOP_KM, followRadiusKm, pad, pointBox, stopRadiusKm, union } from "./camera.ts";
import { project } from "./map.ts";
import { type Center, type WaveSource, currentGroup, setWaveReach, geoOf, groupPlace, groupScale, pendingHindsight, recentQuakes, relatedQuake, shakenGeo, unsettledGroups, waveSources } from "./quakes.ts";
import { sameQuake } from "./priority.ts";
import { forecastTag } from "./detail.ts";
import { heldEew } from "./groups.ts";
import { $, map } from "./dom.ts";
import { REPLAY_SPEED, app, now } from "./state.ts";
import { activeAreas } from "./tsunami.ts";
import { VP_KM_S, VS_KM_S, capKm, surfaceRadiusKm, waveLifeSec } from "./waves.ts";

/** 震央の印。番号ごとに 1 つ (同じ地震の EEW と地震情報は地震情報の震源を使う) */
export function renderMarkers(now: number): void {
  const cur = currentGroup();
  const shown = cur && relatedQuake(cur);
  // 履歴や矢印で別の地震を選んでいる間も、直近のほかの地震の印と矢印は出す (元の地震へ戻れるように)
  const groups = [...recentQuakes(now), ...(shown ? [shown] : [])];
  const byNum = new Map<string, { key: string; lat: number; lon: number; label: number | null; primary: boolean; scale: number; note?: string; ghost?: boolean; quake: boolean }>();
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
    const note = g.kind === "eew" ? (forecastTag(heldEew(g)) ?? undefined) : undefined;
    byNum.set(id, { key: g.key, lat: c.lat, lon: c.lon, label, primary: primary || (prev?.primary ?? false), scale: groupScale(g), note, quake: g.kind === "quake" });
  }
  // 履歴の再生: 震源が届くまでは、後の報で分かった震源を薄く出す
  const h = pendingHindsight(app.demo?.hindsight ?? null, app.world.store.list());
  if (h) byNum.set("hindsight", { key: "hindsight", lat: h.lat, lon: h.lon, label: null, primary: true, scale: 0, note: "のちに判明する震源", ghost: true, quake: false });
  map.setEpicenters([...byNum.values()].map(({ quake: _, ...m }) => m));
}

interface Scene {
  center: Center | null;
  /** カメラの対象以外で波を描く地震 (ライブで複数の地震が重なったとき) */
  others: WaveSource[];
  /** 発生からの秒数。null なら波は描かない */
  t: number | null;
  shaken: Box | null;
  replay: boolean;
  /** 緊急地震速報の予測: 予測の出た地域 (shaken) を最初から収める (S波に合わせて引くのを待たない) */
  forecast?: boolean;
  /** 中心の地震のグループの key (サブの地図が同じ地震か確かめるため) */
  key?: string;
}

/** 揺れた範囲の箱: 揺れた細分区域の和。区域が読めていない・引けないときは県の本土の箱 (離島の地震では本土が映る) */
function shakenBox(geo: { prefs: string[]; areas: string[] }): Box | null {
  return map.areaBox(geo.areas) ?? map.prefBox(geo.prefs);
}

// 波は、その地震の揺れた範囲の一番遠い角まで S 波が届くまで描く。緊急地震速報の波は、同じ地震の地震情報
// (震度速報など、震源の分からないものも) の揺れた範囲も見る。震源だけのときは既定の上限
setWaveReach((s) => {
  const [x, y] = project(s.lon, s.lat);
  const g = s.group;
  const mine = g ? [g, ...app.world.store.list().filter((q) => q.kind === "quake" && q !== g && sameQuake(groupPlace(q), groupPlace(g)))] : [];
  return stopRadiusKm(x, y, mine.map((m) => shakenBox(shakenGeo(m))).reduce<Box | null>((a, b) => union(a, b), null));
});

/** いま地図で見せる地震。null なら日本全体 */
export function scene(now: number): Scene | null {
  if (app.selectedKey) {
    const g = app.world.store.get(app.selectedKey);
    const rq = g && relatedQuake(g);
    const geo = rq && geoOf(rq);
    if (!geo) return null;
    return { center: geo.center, others: [], t: ((now - app.selectedAt) / 1000) * REPLAY_SPEED, shaken: shakenBox(shakenGeo(rq)), replay: true, key: rq.key };
  }
  // 巡回中はその地震に合わせる (ほかの地震の波も描く)
  if (app.tourKey) {
    const g = app.world.store.get(app.tourKey);
    const geo = g && geoOf(g);
    if (geo) {
      const sources = waveSources(now);
      const mine = sources.find((s) => s.group === g || (s.group && sameQuake(groupPlace(s.group), groupPlace(g))));
      return {
        center: mine ?? geo.center,
        others: sources.filter((s) => s !== mine),
        t: mine ? (now - mine.origin) / 1000 : null,
        shaken: shakenBox(shakenGeo(g)),
        replay: false,
        forecast: g.kind === "eew",
        key: g.key,
      };
    }
  }
  // カメラは揺れの大きい方に合わせる
  const [src, ...others] = waveSources(now);
  // 波が終わったら、津波予報が出ていれば予報区全体を見せる
  const tsunamiBox = map.tsunamiBox(activeAreas(app.world.tsunami).map((a) => a.name));
  if (!src && tsunamiBox) return { center: null, others: [], t: null, shaken: tsunamiBox, replay: false };
  const g = src?.group ?? unsettledGroups(now)[0];
  if (!g && !src) {
    // 履歴の再生で、発生前・報が届く前: 後で分かった震源へ寄せる
    const h = pendingHindsight(app.demo?.hindsight ?? null, app.world.store.list());
    return h && { center: h, others: [], t: null, shaken: null, replay: false };
  }
  const geo = g && geoOf(g);
  return {
    center: src ?? geo?.center ?? null,
    others,
    t: src ? (now - src.origin) / 1000 : null,
    shaken: g && geo ? shakenBox(shakenGeo(g)) : null,
    replay: false,
    forecast: g?.kind === "eew",
    key: g?.key,
  };
}

/** 主の地図に描いた中心の地震の波 (サブの地図へ渡す) */
export interface MainWave {
  key: string;
  lat: number;
  lon: number;
  pKm: number | null;
  sKm: number | null;
}

/** 波を描き、カメラの目標を返す。波を描いているかどうかと、中心の地震の波も返す */
export function renderScene(sc: Scene | null): { box: Box | null; waving: boolean; wave: MainWave | null } {
  if (!sc?.center) return { box: sc?.shaken ? pad(sc.shaken) : null, waving: false, wave: null };
  const c = sc.center;
  const [x, y] = project(c.lon, c.lat);
  const stop = stopRadiusKm(x, y, sc.shaken);
  const s = sc.t == null ? null : surfaceRadiusKm(VS_KM_S, c.depth, sc.t);
  // 波は S 波が揺れた範囲の端に届くまで (waveLifeSec)。再生は揺れた地域を覆い終えたら打ち切る (早回しでも 180 秒分は長い)
  const waving = sc.t != null && sc.t < waveLifeSec(c.depth, stop) && !(sc.replay && (s ?? 0) > stop);
  if (!waving) return { box: pad(union(sc.shaken, pointBox(x, y, 0))!), waving, wave: null };
  // 円は寄りが引く上限 (揺れた範囲の一番遠い角) で止める。ほかの地震は揺れた範囲を持たないので既定の上限
  const wave = (w: Center, t: number, maxKm: number) => ({ ...w, pKm: capKm(surfaceRadiusKm(VP_KM_S, w.depth, t), maxKm), sKm: capKm(surfaceRadiusKm(VS_KM_S, w.depth, t), maxKm) });
  const main = wave(c, sc.t!, stop);
  map.setWaves([main, ...sc.others.map((o) => wave(o, (now() - o.origin) / 1000, DEFAULT_STOP_KM))]);
  $("#wave-info").textContent = sc.replay ? `再生中 ${sc.t!.toFixed(0)}秒 (×${REPLAY_SPEED})` : `発生から${sc.t!.toFixed(0)}秒`;
  const follow = pointBox(x, y, followRadiusKm(s, stop));
  const sub = sc.key ? { key: sc.key, lat: main.lat, lon: main.lon, pKm: main.pKm, sKm: main.sKm } : null;
  return { box: pad(sc.forecast ? union(follow, sc.shaken)! : follow), waving, wave: sub };
}
