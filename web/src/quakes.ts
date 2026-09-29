// 地震の選び方: 優先度・一時的な番号・P波/S波を描く対象・直近の地震 (DOM には触らない)。

import { FULL_MS } from "./fade.ts";
import { type Group, latestEew, summarizeQuake } from "./groups.ts";
import { assignNumbers } from "./numbering.ts";
import { type Place, byPriority, sameQuake, settleMs } from "./priority.ts";
import { EEW_BANNER_MS, WAVE_MAX_SEC, app, now } from "./state.ts";
import { tourIndex, worthTouring } from "./tour.ts";
import type { EewEvent, Hypocenter, Scale } from "./types.ts";

/** 津波予報などに対応する地震 (その情報より前に届いた直近の地震情報・EEW) */
export function relatedQuake(g: Group): Group | undefined {
  if (g.kind === "quake" || g.kind === "eew") return g;
  return app.world.store.list().find((q) => (q.kind === "quake" || q.kind === "eew") && q.updatedAt <= g.updatedAt);
}

/** 直近の地震 (起きた順) */
export function recentQuakes(now: number): Group[] {
  const origin = (g: Group) => geoOf(g)?.origin ?? g.updatedAt;
  return app.world.store
    .list()
    .filter((g) => (g.kind === "quake" || g.kind === "eew") && geoOf(g) && now - g.updatedAt <= FULL_MS)
    .sort((a, b) => origin(a) - origin(b));
}

/** 番号を振り直す。変わったら true */
export function updateNumbers(now: number): boolean {
  const recent = recentQuakes(now);
  const eews = recent.filter((g) => g.kind === "eew");
  const linkOf = (g: Group) => {
    if (g.kind !== "quake") return undefined;
    const p = groupPlace(g);
    const near = eews.filter((e) => sameQuake(groupPlace(e), p));
    // 群発で候補が複数あれば発生時刻の近いもの
    near.sort((a, b) => Math.abs(groupPlace(a).originMs! - p.originMs!) - Math.abs(groupPlace(b).originMs! - p.originMs!));
    return near[0]?.key;
  };
  const next = assignNumbers(
    app.numbers,
    recent.map((g) => ({ key: g.key, linkedTo: linkOf(g) })),
  );
  const changed = JSON.stringify([...next]) !== JSON.stringify([...app.numbers]);
  app.numbers = next;
  return changed;
}

/** 地図に塗っている地震の発生時刻 (無ければ受信時刻) */
/** 表示中の地震の塗りを薄くし始める起点: 最後の情報が届いた時刻 (震度が後から届いても、届いた震度は見えるように) */
export function displayedInfoMs(): number {
  const g = currentGroup();
  const q = g && relatedQuake(g);
  return q ? q.updatedAt : 0;
}

export function currentGroup(): Group | undefined {
  if (app.selectedKey) return app.world.store.get(app.selectedKey);
  if (app.tourKey) return app.world.store.get(app.tourKey);
  return priorityGroups(now())[0] ?? app.world.store.list().find((g) => g.kind === "quake" || g.kind === "eew" || g.kind === "tsunami");
}

/** バナーを出している EEW (新しい順) */
export function activeEews(now: number): EewEvent[] {
  return app.world.store
    .list()
    .filter((g) => g.kind === "eew")
    .map((g) => latestEew(g))
    .filter((e) => !e.cancelled && now - e.received_at_ms < EEW_BANNER_MS && e.received_at_ms > app.calmSince);
}

export function placeOf(origin: number | null, h: Hypocenter | null): Place {
  return { originMs: origin, lat: h?.latitude ?? null, lon: h?.longitude ?? null };
}

export function groupPlace(g: Group): Place {
  const geo = geoOf(g);
  return { originMs: geo?.origin ?? null, lat: geo?.center?.lat ?? null, lon: geo?.center?.lon ?? null };
}

export function groupScale(g: Group): Scale {
  if (g.kind === "quake") return summarizeQuake(g).maxScale;
  if (g.kind === "eew") return latestEew(g).max_scale;
  return -1;
}

/**
 * 最近 (FULL_MS 以内) の地震を優先度順に。揺れの大きい方 (EEW は予測、地震情報は観測) が先、同じなら新しい方。
 * 地震情報が届いた EEW はその地震情報に任せる (予測の震度で居座らないように)
 */
/** まだ表示を続ける地震 (最後の情報から settleMs 以内。軽い地震は短い) */
export function unsettledGroups(now: number): Group[] {
  return priorityGroups(now).filter((g) => now - g.updatedAt <= settleMs(groupScale(g)));
}

export function priorityGroups(now: number): Group[] {
  // 「警報・注意報」ボタンで平時に戻した後は、それ以前の地震は優先して見せない
  const recent = app.world.store
    .list()
    .filter((g) => (g.kind === "quake" || g.kind === "eew") && geoOf(g) && now - g.updatedAt <= FULL_MS && g.updatedAt > app.calmSince);
  const quakes = recent.filter((g) => g.kind === "quake").map(groupPlace);
  return recent
    .filter((g) => g.kind === "quake" || !quakes.some((q) => sameQuake(groupPlace(g), q)))
    .map((g) => ({ g, scale: groupScale(g), at: g.updatedAt }))
    .sort(byPriority)
    .map((c) => c.g);
}

export interface Center {
  lat: number;
  lon: number;
  depth: number;
}

/** 地震 (地震情報・EEW) のグループから震源・発生時刻・揺れた都道府県を取り出す */
export function geoOf(g: Group): { center: Center | null; origin: number | null; prefs: string[] } | null {
  let h: Hypocenter | null;
  let origin: number | null;
  let prefs: string[];
  if (g.kind === "eew") {
    const e = latestEew(g);
    if (e.cancelled) return null;
    [h, origin, prefs] = [e.hypocenter, e.origin_time_ms, e.pref_max.map((p) => p.pref)];
  } else if (g.kind === "quake") {
    const q = summarizeQuake(g);
    [h, origin, prefs] = [q.hypocenter, q.originTimeMs, q.prefMax.map((p) => p.pref)];
  } else return null;
  const center = h?.latitude != null && h.longitude != null ? { lat: h.latitude, lon: h.longitude, depth: h.depth_km ?? 10 } : null;
  return { center, origin, prefs };
}

export type WaveSource = Center & { origin: number; group: Group };

/**
 * P波・S波を描く地震 (優先度順)。同じ地震の EEW と地震情報は EEW を使う
 * (地震情報の発生時刻は分単位なので、円が遅れて見える)
 */
export function waveSources(now: number): WaveSource[] {
  const out: WaveSource[] = [];
  const groups = app.world.store.list().filter((g) => g.kind === "eew" || g.kind === "quake");
  for (const g of [...groups.filter((g) => g.kind === "eew"), ...groups.filter((g) => g.kind === "quake")]) {
    const geo = geoOf(g);
    if (!geo?.center || geo.origin == null || now - geo.origin > WAVE_MAX_SEC * 1000 || g.updatedAt <= app.calmSince) continue;
    // EEW どうしは event_id で別の地震と分かっているので、重ねて消すのは地震情報だけ
    if (g.kind === "quake" && out.some((s) => s.group.kind === "eew" && sameQuake(groupPlace(s.group), groupPlace(g)))) continue;
    out.push({ ...geo.center, origin: geo.origin, group: g });
  }
  return out
    .map((s) => ({ s, scale: groupScale(s.group), at: s.group.updatedAt }))
    .sort(byPriority)
    .map((c) => c.s);
}

/**
 * 巡回で今見せる地震を決める (app.tourKey)。離れた場所で 2 つ以上起きているときだけ、番号順に設定の間隔で切り替える。
 * manual は利用者が地図を動かしているか (そのときは巡回しない)
 */
export function updateTour(now: number, manual: boolean): void {
  const hold = app.tourHold && now < app.tourHold.until && app.world.store.get(app.tourHold.key) ? app.tourHold.key : null;
  const cands = unsettledGroups(now)
    .map((g) => ({ g, c: geoOf(g)?.center }))
    .filter((x): x is { g: Group; c: NonNullable<typeof x.c> } => x.c != null)
    .sort((a, b) => (app.numbers.get(a.g.key) ?? 0) - (app.numbers.get(b.g.key) ?? 0));
  if (app.selectedKey || manual || app.settings.tourSec === 0 || !worthTouring(cands.map((x) => x.c))) {
    app.tourStart = null;
    app.tourKey = null;
    return;
  }
  app.tourStart ??= now;
  app.tourKey = hold ?? cands[tourIndex(app.tourStart, now, app.settings.tourSec, cands.length)].g.key;
}
