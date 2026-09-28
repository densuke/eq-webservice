import { Connection, type Status } from "./connection.ts";
import { GroupStore, latestEew, summarizeQuake, type Group } from "./groups.ts";
import { alertLevel, type AlertLevel } from "./alert.ts";
import { followRadiusKm, pad, pointBox, stopRadiusKm, union, type Box } from "./camera.ts";
import { fadeOpacity, FULL_MS } from "./fade.ts";
import { JapanMap, project } from "./map.ts";
import { isKnownScale, scaleColor, scaleLabel, scaleTextColor } from "./scale.ts";
import { activeAreas, latestTsunami, tsunamiAlert } from "./tsunami.ts";
import type { EewEvent, EqEvent, Hypocenter, Scale, TsunamiEvent } from "./types.ts";
import { play, setSoundEnabled, soundEnabled, soundReady, unlock } from "./sound.ts";
import { surfaceRadiusKm, VP_KM_S, VS_KM_S } from "./waves.ts";
import { byPriority, sameQuake, type Place } from "./priority.ts";
import { assignNumbers } from "./numbering.ts";
import { clockParts } from "./clock.ts";

/** 発生からこの秒数を過ぎたら P波・S波の表示を止める */
const WAVE_MAX_SEC = 180;
/** EEW 警報バナーを出し続ける時間 */
const EEW_BANNER_MS = 3 * 60_000;
/** テロップの文を切り替える間隔 */
const TELOP_INTERVAL_MS = 8000;
/** EEW バナーに並べる件数 (残りは「ほか N 件」) */
const EEW_BANNER_MAX = 3;
/** この時間内に続けて届いた情報では、前より強い音のときだけ鳴らす */
const ALERT_MERGE_MS = 3000;
/** 履歴を選んだときの P波・S波の再生速度 */
const REPLAY_SPEED = 3;

const $ = <T extends HTMLElement>(sel: string) => document.querySelector(sel) as T;

const store = new GroupStore();
const map = new JapanMap($("#map"));
let selectedKey: string | null = null; // null は「最新に自動追従」
let selectedAt = 0; // 履歴を選んだ時刻 (再生の起点)
/** 受け取った最新の津波予報 (解除を含む)。一覧の整理で消えないよう別に持つ */
let tsunami: TsunamiEvent | null = null;
/** 直近の地震の一時的な番号 (グループのキー → 番号) */
let numbers = new Map<string, number>();
let conn: Connection;

// ---------- 描画ヘルパ ----------

function esc(s: string): string {
  return s.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}

function numTag(key: string): string {
  const n = numbers.get(key);
  return n == null ? "" : `<span class="num">${n}</span>`;
}

function badge(s: Scale, big = false): string {
  return `<span class="badge${big ? " big" : ""}" style="background:${scaleColor(s)};color:${scaleTextColor(s)}">${
    isKnownScale(s) ? scaleLabel(s) : "-"
  }</span>`;
}

function hypoText(h: Hypocenter | null): string {
  if (!h) return "震源調査中";
  const parts = [esc(h.name || "震源不明")];
  if (h.magnitude != null) parts.push(`M${h.magnitude.toFixed(1)}`);
  if (h.depth_km === 0) parts.push("ごく浅い");
  else if (h.depth_km != null) parts.push(`深さ${h.depth_km}km`);
  return parts.join(" / ");
}

const TSUNAMI_TEXT: Record<string, string> = {
  None: "この地震による津波の心配はありません",
  NonEffective: "若干の海面変動 (被害の心配なし)",
  Checking: "津波の有無を調査中",
  Watch: "津波注意報 発表中",
  Warning: "津波警報等 発表中",
};

const GRADE_LABEL: Record<string, string> = {
  major_warning: "大津波警報",
  warning: "津波警報",
  watch: "津波注意報",
  unknown: "不明",
};

// ---------- 一覧 ----------

function groupRow(g: Group): string {
  switch (g.kind) {
    case "quake": {
      const q = summarizeQuake(g);
      return `${badge(q.maxScale)}<div class="row-main"><div class="row-title">${numTag(g.key)}${esc(
        q.hypocenter?.name || "震源調査中",
      )}</div><div class="row-sub">${esc(q.originTime.slice(5, 16))} ${q.hypocenter?.magnitude != null ? "M" + q.hypocenter.magnitude.toFixed(1) : ""} ・${q.infoLabel}</div></div>`;
    }
    case "eew": {
      const e = latestEew(g);
      return `${badge(e.max_scale)}<div class="row-main"><div class="row-title eew-title">${numTag(g.key)}${e.test ? "[テスト] " : ""}緊急地震速報${e.warning ? "" : " (予報)"} ${
        e.cancelled ? "(取消)" : esc(e.hypocenter?.name ?? "")
      }</div><div class="row-sub">${esc((e.origin_time ?? e.issued_at).slice(5, 16))} ・第${esc(e.serial)}報</div></div>`;
    }
    case "tsunami": {
      const t = g.events[0] as TsunamiEvent;
      const top = t.areas[0]?.grade ?? "unknown";
      return `<span class="badge tsunami ${top}">津</span><div class="row-main"><div class="row-title">${
        t.cancelled ? "津波予報 解除" : GRADE_LABEL[top]
      }</div><div class="row-sub">${esc(t.issued_at.slice(5, 16))} ・${t.areas.length}地域</div></div>`;
    }
    case "eew_detection":
      return `<span class="badge">!</span><div class="row-main"><div class="row-title">緊急地震速報 発表検出</div><div class="row-sub">${new Date(
        g.updatedAt,
      ).toLocaleTimeString("ja-JP", { timeZone: "Asia/Tokyo" })}</div></div>`;
  }
}

function renderList(): void {
  const list = $("#list");
  const groups = store.list().filter((g) => g.kind !== "eew_detection");
  const current = currentGroup();
  list.innerHTML = groups
    .slice(0, 100)
    .map((g) => `<li data-key="${esc(g.key)}" class="${g.key === current?.key ? "selected" : ""}">${groupRow(g)}</li>`)
    .join("");
  renderFollow();
}

function renderFollow(): void {
  const following = selectedKey === null && !map.userMoved;
  const btn = $("#follow");
  const text = following ? "リアルタイム表示中" : "リアルタイムに戻る";
  if (btn.textContent === text) return;
  btn.textContent = text;
  btn.classList.toggle("active", following);
}

// ---------- 詳細 ----------

/** 津波予報などに対応する地震 (その情報より前に届いた直近の地震情報・EEW) */
function relatedQuake(g: Group): Group | undefined {
  if (g.kind === "quake" || g.kind === "eew") return g;
  return store.list().find((q) => (q.kind === "quake" || q.kind === "eew") && q.updatedAt <= g.updatedAt);
}

/** 地図の塗り分けと震央 */
function paintMap(g: Group | undefined): void {
  if (g?.kind === "quake") {
    const q = summarizeQuake(g);
    map.setPrefScales(q.prefMax);
  } else if (g?.kind === "eew") {
    const e = latestEew(g);
    map.setPrefScales(e.cancelled ? [] : e.pref_max, true);
  } else {
    map.setPrefScales([]);
  }
}

/** 直近の地震 (起きた順) */
function recentQuakes(now: number): Group[] {
  const origin = (g: Group) => geoOf(g)?.origin ?? g.updatedAt;
  return store
    .list()
    .filter((g) => (g.kind === "quake" || g.kind === "eew") && geoOf(g) && now - g.updatedAt <= FULL_MS)
    .sort((a, b) => origin(a) - origin(b));
}

/** 番号を振り直す。変わったら true */
function updateNumbers(now: number): boolean {
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
    numbers,
    recent.map((g) => ({ key: g.key, linkedTo: linkOf(g) })),
  );
  const changed = JSON.stringify([...next]) !== JSON.stringify([...numbers]);
  numbers = next;
  return changed;
}

/** 震央の印。番号ごとに 1 つ (同じ地震の EEW と地震情報は地震情報の震源を使う) */
function renderMarkers(now: number): void {
  const cur = currentGroup();
  const shown = cur && relatedQuake(cur);
  const groups = selectedKey ? (shown ? [shown] : []) : [...recentQuakes(now), ...(shown ? [shown] : [])];
  const byNum = new Map<string, { lat: number; lon: number; label: number | null; primary: boolean; quake: boolean }>();
  for (const g of groups) {
    const c = geoOf(g)?.center;
    if (!c) continue;
    const label = numbers.get(g.key) ?? null;
    const id = label == null ? g.key : String(label);
    const primary = g === shown || (label != null && shown != null && numbers.get(shown.key) === label);
    const prev = byNum.get(id);
    if (prev && (prev.quake || g.kind !== "quake")) {
      prev.primary ||= primary;
      continue;
    }
    byNum.set(id, { lat: c.lat, lon: c.lon, label, primary: primary || (prev?.primary ?? false), quake: g.kind === "quake" });
  }
  map.setEpicenters([...byNum.values()].map(({ quake: _, ...m }) => m));
}

/** 地図に塗っている地震の発生時刻 (無ければ受信時刻) */
function displayedOriginMs(): number {
  const g = currentGroup();
  const q = g && relatedQuake(g);
  if (!q) return 0;
  return geoOf(q)?.origin ?? q.updatedAt;
}

function currentGroup(): Group | undefined {
  if (selectedKey) return store.get(selectedKey);
  return priorityGroups(now())[0] ?? store.list().find((g) => g.kind === "quake" || g.kind === "eew" || g.kind === "tsunami");
}

function renderDetail(): void {
  const g = currentGroup();
  const box = $("#detail");
  paintMap(g && relatedQuake(g));
  if (!g) {
    box.innerHTML = `<p class="muted">受信した情報はまだありません。</p>`;
    return;
  }
  if (g.kind === "quake") {
    const q = summarizeQuake(g);
    // 震度の大きい順に観測点をまとめる
    const byScale = new Map<Scale, Map<string, string[]>>();
    for (const p of q.points) {
      const prefs = byScale.get(p.scale) ?? new Map<string, string[]>();
      byScale.set(p.scale, prefs);
      prefs.set(p.pref, [...(prefs.get(p.pref) ?? []), p.addr]);
    }
    const scales = [...byScale.keys()].sort((a, b) => b - a);
    box.innerHTML = `
      <div class="detail-head">${badge(q.maxScale, true)}
        <div><div class="detail-kind">${q.infoLabel}</div>
        <div class="detail-title">${numTag(g.key)}${esc(q.hypocenter?.name || "震源調査中")}</div>
        <div class="detail-sub">${esc(q.originTime)} 発生</div></div></div>
      <dl class="facts">
        <dt>震源</dt><dd>${hypoText(q.hypocenter)}</dd>
        <dt>津波</dt><dd>${esc(TSUNAMI_TEXT[q.domesticTsunami] ?? "—")}</dd>
      </dl>
      ${q.comment ? `<p class="comment">${esc(q.comment)}</p>` : ""}
      <div class="points">${scales
        .map(
          (s) =>
            `<div class="point-row">${badge(s)}<div>${[...byScale.get(s)!.entries()]
              .map(([pref, addrs]) => `<b>${esc(pref)}</b> ${esc(addrs.slice(0, 30).join("、"))}${addrs.length > 30 ? " ほか" : ""}`)
              .join("<br>")}</div></div>`,
        )
        .join("")}</div>`;
  } else if (g.kind === "eew") {
    const e = latestEew(g);
    box.innerHTML = `
      <div class="detail-head">${badge(e.max_scale, true)}
        <div><div class="detail-kind eew-title">緊急地震速報 (${e.warning ? "警報" : "予報"})${e.test ? " [テスト]" : ""} 第${esc(e.serial)}報</div>
        <div class="detail-title">${numTag(g.key)}${e.cancelled ? "取り消されました" : esc(e.hypocenter?.name ?? "震源不明")}</div>
        <div class="detail-sub">${esc(e.origin_time ?? e.issued_at)} 発生</div></div></div>
      <dl class="facts"><dt>震源</dt><dd>${hypoText(e.hypocenter)}</dd><dt>予測最大</dt><dd>震度${scaleLabel(e.max_scale)}</dd></dl>
      <div class="points">${e.areas
        .map(
          (a) =>
            `<div class="point-row">${badge(a.scale_from)}<div><b>${esc(a.name)}</b> 震度${scaleLabel(a.scale_from)}${
              a.scale_to == null ? "程度以上" : a.scale_to !== a.scale_from ? "〜" + scaleLabel(a.scale_to) : ""
            }${a.arrived ? ' <span class="arrived">到達と推測</span>' : ""}</div></div>`,
        )
        .join("")}</div>`;
  } else if (g.kind === "tsunami") {
    const t = g.events[0] as TsunamiEvent;
    box.innerHTML = `<div class="detail-head"><span class="badge big tsunami ${t.areas[0]?.grade ?? "unknown"}">津</span>
      <div><div class="detail-kind">津波予報</div><div class="detail-title">${t.cancelled ? "解除" : GRADE_LABEL[t.areas[0]?.grade ?? "unknown"]}</div>
      <div class="detail-sub">${esc(t.issued_at)} 発表</div></div></div>
      <div class="points">${t.areas
        .map(
          (a) =>
            `<div class="point-row"><span class="badge tsunami ${a.grade}">${GRADE_LABEL[a.grade].replace("津波", "")}</span><div><b>${esc(a.name)}</b>${
              a.immediate ? ' <span class="arrived">直ちに来襲</span>' : ""
            }${a.max_height ? ` 予想 ${esc(a.max_height)}` : ""}${a.first_height ? ` / ${esc(a.first_height)}` : ""}</div></div>`,
        )
        .join("")}</div>`;
  }
}

// ---------- 緊急地震速報のバナーと P波・S波 ----------

function now(): number {
  return conn ? conn.now() : Date.now();
}

/** バナーを出している EEW (新しい順) */
function activeEews(now: number): EewEvent[] {
  return store
    .list()
    .filter((g) => g.kind === "eew")
    .map((g) => latestEew(g))
    .filter((e) => !e.cancelled && now - e.received_at_ms < EEW_BANNER_MS);
}

function placeOf(origin: number | null, h: Hypocenter | null): Place {
  return { originMs: origin, lat: h?.latitude ?? null, lon: h?.longitude ?? null };
}

function groupPlace(g: Group): Place {
  const geo = geoOf(g);
  return { originMs: geo?.origin ?? null, lat: geo?.center?.lat ?? null, lon: geo?.center?.lon ?? null };
}

function groupScale(g: Group): Scale {
  if (g.kind === "quake") return summarizeQuake(g).maxScale;
  if (g.kind === "eew") return latestEew(g).max_scale;
  return -1;
}

/**
 * 最近 (FULL_MS 以内) の地震を優先度順に。揺れの大きい方 (EEW は予測、地震情報は観測) が先、同じなら新しい方。
 * 地震情報が届いた EEW はその地震情報に任せる (予測の震度で居座らないように)
 */
function priorityGroups(now: number): Group[] {
  const recent = store.list().filter((g) => (g.kind === "quake" || g.kind === "eew") && geoOf(g) && now - g.updatedAt <= FULL_MS);
  const quakes = recent.filter((g) => g.kind === "quake").map(groupPlace);
  return recent
    .filter((g) => g.kind === "quake" || !quakes.some((q) => sameQuake(groupPlace(g), q)))
    .map((g) => ({ g, scale: groupScale(g), at: g.updatedAt }))
    .sort(byPriority)
    .map((c) => c.g);
}

interface Center {
  lat: number;
  lon: number;
  depth: number;
}

/** 地震 (地震情報・EEW) のグループから震源・発生時刻・揺れた都道府県を取り出す */
function geoOf(g: Group): { center: Center | null; origin: number | null; prefs: string[] } | null {
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

type WaveSource = Center & { origin: number; group: Group };

/**
 * P波・S波を描く地震 (優先度順)。同じ地震の EEW と地震情報は EEW を使う
 * (地震情報の発生時刻は分単位なので、円が遅れて見える)
 */
function waveSources(now: number): WaveSource[] {
  const out: WaveSource[] = [];
  const groups = store.list().filter((g) => g.kind === "eew" || g.kind === "quake");
  for (const g of [...groups.filter((g) => g.kind === "eew"), ...groups.filter((g) => g.kind === "quake")]) {
    const geo = geoOf(g);
    if (!geo?.center || geo.origin == null || now - geo.origin > WAVE_MAX_SEC * 1000) continue;
    // EEW どうしは event_id で別の地震と分かっているので、重ねて消すのは地震情報だけ
    if (g.kind === "quake" && out.some((s) => s.group.kind === "eew" && sameQuake(groupPlace(s.group), groupPlace(g)))) continue;
    out.push({ ...geo.center, origin: geo.origin, group: g });
  }
  return out
    .map((s) => ({ s, scale: groupScale(s.group), at: s.group.updatedAt }))
    .sort(byPriority)
    .map((c) => c.s);
}

interface Scene {
  center: Center | null;
  /** カメラの対象以外で波を描く地震 (ライブで複数の地震が重なったとき) */
  others: WaveSource[];
  /** 発生からの秒数。null なら波は描かない */
  t: number | null;
  shaken: Box | null;
  replay: boolean;
}

/** いま地図で見せる地震。null なら日本全体 */
function scene(now: number): Scene | null {
  if (selectedKey) {
    const g = store.get(selectedKey);
    const geo = g && relatedQuake(g) && geoOf(relatedQuake(g)!);
    if (!geo) return null;
    return { center: geo.center, others: [], t: ((now - selectedAt) / 1000) * REPLAY_SPEED, shaken: map.prefBox(geo.prefs), replay: true };
  }
  // カメラは揺れの大きい方に合わせる
  const [src, ...others] = waveSources(now);
  // 波が終わったら、津波予報が出ていれば予報区全体を見せる
  const tsunamiBox = map.tsunamiBox(activeAreas(tsunami).map((a) => a.name));
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
function renderScene(sc: Scene | null): { box: Box | null; waving: boolean } {
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

function renderTsunamiBanner(): void {
  const areas = activeAreas(tsunami);
  const banner = $("#tsunami-banner");
  banner.hidden = areas.length === 0;
  if (areas.length === 0) return;
  const grades = (["major_warning", "warning", "watch"] as const).filter((g) => areas.some((a) => a.grade === g));
  banner.dataset.grade = grades[0] ?? "unknown";
  banner.innerHTML = grades
    .map((g) => `<b>${GRADE_LABEL[g]}</b> ${esc(areas.filter((a) => a.grade === g).map((a) => a.name).join("・"))}`)
    .join(" ／ ");
}

function renderBanner(now: number): void {
  // 揺れの大きい順に数件だけ並べる
  const eews = activeEews(now)
    .map((e) => ({ e, scale: e.max_scale, at: e.received_at_ms }))
    .sort(byPriority)
    .map((c) => c.e);
  const banner = $("#eew-banner");
  banner.hidden = eews.length === 0;
  // 予報だけなら警報と色を分ける
  banner.classList.toggle("forecast", eews.length > 0 && eews.every((e) => !e.warning));
  const rest = eews.length - EEW_BANNER_MAX;
  banner.innerHTML =
    eews
      .slice(0, EEW_BANNER_MAX)
      .map((e) => {
        const prefs = e.pref_max.map((p) => p.pref).join("・");
        return `<div>${numTag(`e:${e.event_id}`)}<b>${e.test ? "【テスト】" : ""}緊急地震速報 (${e.warning ? "警報" : "予報"})</b> ${esc(e.hypocenter?.name ?? "")} で地震 ・ ${
          e.warning ? "強い揺れに警戒" : `予測最大震度${scaleLabel(e.max_scale)}`
        }: ${esc(prefs || "—")}</div>`;
      })
      .join("") + (rest > 0 ? `<div>ほか ${rest} 件の緊急地震速報</div>` : "");
}

let raf = 0;
let lastPip = -1;
let lastCurrentKey: string | undefined;
/** 直前に鳴らした警戒音 (数秒以内に重なったら強い方だけ鳴らす) */
let lastAlert = { level: "info" as AlertLevel, at: 0 };
let timer = 0;
function tick(): void {
  cancelAnimationFrame(raf);
  clearTimeout(timer);
  const now = conn.now();
  renderClock(now);
  if (updateNumbers(now)) {
    renderList();
    renderDetail();
  }
  renderBanner(now);
  renderMarkers(now);
  const sc = scene(now);
  const { box, waving } = renderScene(sc);
  // 波の広がり中 (ライブのみ) は 2 秒ごとに短い音で警戒中を知らせる (地震が重なっても 1 本)
  const pip = waving && !sc!.replay ? Math.floor(now / 2000) : -1;
  if (pip > lastPip && lastPip !== -1) play("pip");
  lastPip = pip;
  // 優先度は時間で入れ替わる (大きい方が古くなるなど) ので、表示中の地震が変わったら描き直す
  const key = currentGroup()?.key;
  if (key !== lastCurrentKey) {
    lastCurrentKey = key;
    renderList();
    renderDetail();
  }
  $("#legend-wave").hidden = !waving;
  renderTelop(now, waving || activeEews(now).length > 0 || priorityGroups(now).length > 0 || activeAreas(tsunami).length > 0);
  if (!waving) {
    map.setWaves([]);
    $("#wave-info").textContent = "";
  }
  map.setTarget(box);
  map.setFade(selectedKey ? 1 : fadeOpacity(now - displayedOriginMs()));
  renderFollow();
  renderSound();
  // 波の表示中は滑らかに、そうでなければ時計の更新だけ
  if (waving) raf = requestAnimationFrame(tick);
  else timer = window.setTimeout(tick, 1000);
}

// ---------- イベント受信 ----------

function onEvents(events: EqEvent[], live: boolean): void {
  const rank: Record<AlertLevel, number> = { info: 0, low: 1, medium: 2, strong: 3 };
  let alert: AlertLevel | null = null;
  for (const e of events) {
    const g = store.add(e);
    if (!g) continue;
    if (e.kind === "tsunami") {
      const prev = activeAreas(tsunami);
      tsunami = latestTsunami(tsunami, e);
      const lv = live ? tsunamiAlert(prev, activeAreas(tsunami)) : null;
      if (lv && (!alert || rank[lv] > rank[alert])) alert = lv;
    }
    if (!live) continue;
    // その地震の EEW で既に鳴らしていれば、地震情報では鳴らさない
    const eewActive =
      e.kind === "quake" && activeEews(now()).some((x) => sameQuake(placeOf(x.origin_time_ms, x.hypocenter), placeOf(e.origin_time_ms, e.hypocenter)));
    // EEW は予報から警報に上がったときも鳴らす
    const isNew =
      g.events.length === 1 || (e.kind === "eew" && e.warning && !g.events.slice(0, -1).some((x) => (x as EewEvent).warning));
    const lv = alertLevel(e, isNew, eewActive);
    if (lv && (!alert || rank[lv] > rank[alert])) alert = lv;
  }
  renderList();
  renderDetail();
  // Wolfx 経由の情報を受けたら出典を出す
  if (events.some((e) => e.source === "wolfx")) $("#credit-wolfx").hidden = false;
  map.setTsunami(activeAreas(tsunami));
  $("#legend-tsunami").hidden = activeAreas(tsunami).length === 0;
  renderTsunamiBanner();
  if (alert && !(now() - lastAlert.at < ALERT_MERGE_MS && rank[alert] <= rank[lastAlert.level])) {
    play(alert);
    lastAlert = { level: alert, at: now() };
  }
  tick();
}

let telopMessages: string[] = [];

/** テロップ。地震の情報を出している間は邪魔をしないよう消す */
function renderTelop(now: number, busy: boolean): void {
  const el = $("#telop");
  el.hidden = busy || telopMessages.length === 0;
  if (el.hidden) return;
  const slot = Math.floor(now / TELOP_INTERVAL_MS);
  const text = telopMessages[slot % telopMessages.length];
  if (el.textContent === text) return;
  // 切り替え時は一度消してから出す
  el.classList.add("fading");
  setTimeout(() => {
    el.textContent = text;
    el.classList.remove("fading");
  }, 400);
}

function loadTelop(): void {
  fetch("api/telop")
    .then((r) => (r.ok ? r.json() : []))
    .then((m: unknown) => {
      if (Array.isArray(m)) telopMessages = m.filter((x): x is string => typeof x === "string");
    })
    .catch(() => {});
}

function renderClock(now: number): void {
  const c = clockParts(now);
  const set = (id: string, text: string) => {
    const el = $(id);
    if (el.textContent !== text) el.textContent = text;
  };
  set("#c-year", `${c.year}年`);
  set("#c-month", `${c.month}月`);
  set("#c-day", `${c.day}日`);
  set("#c-wd", `(${c.weekday})`);
  set("#c-hm", c.hm);
  set("#c-sec", c.sec);
}

function setStatus(s: Status): void {
  $("#clock").dataset.status = s;
  $("#c-status").textContent = { connecting: "接続中", open: "時刻同期", closed: "切断中" }[s];
}

// ---------- 起動 ----------

$("#list").addEventListener("click", (e) => {
  const li = (e.target as HTMLElement).closest("li");
  if (!li) return;
  selectedKey = li.dataset.key ?? null;
  selectedAt = conn.now();
  map.release();
  renderList();
  renderDetail();
});
$("#follow").addEventListener("click", () => {
  selectedKey = null;
  map.release();
  renderList();
  renderDetail();
  tick();
});

function renderSound(): void {
  const btn = $("#sound");
  const text = !soundEnabled() ? "音 OFF" : soundReady() ? "音 ON" : "音 ON (タップで有効化)";
  if (btn.textContent === text) return;
  btn.textContent = text;
  btn.classList.toggle("active", soundEnabled() && soundReady());
}
$("#sound").addEventListener("click", () => {
  setSoundEnabled(!soundEnabled());
  if (soundEnabled()) {
    unlock();
    play("low"); // 確認用
  }
  renderSound();
});
// 前回 ON にしていた場合、ブラウザの制約で最初の操作までは鳴らせない
if (soundEnabled()) document.addEventListener("pointerdown", unlock, { once: true });

loadTelop();
Promise.all([
  map.load("japan.geojson"),
  // 無くても地震の表示はできる
  map.loadTsunami("tsunami.geojson").catch(() => {}),
])
  .catch((err) => {
    $("#detail").innerHTML = `<p class="error">${esc(String(err))}</p>`;
  })
  .finally(() => {
    conn = new Connection(Connection.defaultUrl(), {
      onSnapshot: (events) => onEvents(events, false),
      onEvent: (event) => onEvents([event], true),
      onStatus: setStatus,
    });
    conn.start();
    tick();
  });
