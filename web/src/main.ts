import { Connection, type Status } from "./connection.ts";
import { GroupStore, latestEew, summarizeQuake, type Group } from "./groups.ts";
import { alertLevel, type AlertLevel } from "./alert.ts";
import { followRadiusKm, pad, pointBox, stopRadiusKm, union, type Box } from "./camera.ts";
import { fadeOpacity, FULL_MS } from "./fade.ts";
import { JapanMap, project } from "./map.ts";
import { isKnownScale, scaleColor, scaleLabel, scaleTextColor } from "./scale.ts";
import type { EewEvent, EqEvent, Hypocenter, Scale, TsunamiEvent } from "./types.ts";
import { play, setSoundEnabled, soundEnabled, soundReady, unlock } from "./sound.ts";
import { surfaceRadiusKm, VP_KM_S, VS_KM_S } from "./waves.ts";

/** 発生からこの秒数を過ぎたら P波・S波の表示を止める */
const WAVE_MAX_SEC = 180;
/** EEW 警報バナーを出し続ける時間 */
const EEW_BANNER_MS = 3 * 60_000;
/** 履歴を選んだときの P波・S波の再生速度 */
const REPLAY_SPEED = 3;

const $ = <T extends HTMLElement>(sel: string) => document.querySelector(sel) as T;

const store = new GroupStore();
const map = new JapanMap($("#map"));
let selectedKey: string | null = null; // null は「最新に自動追従」
let selectedAt = 0; // 履歴を選んだ時刻 (再生の起点)
let conn: Connection;

// ---------- 描画ヘルパ ----------

function esc(s: string): string {
  return s.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
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
      return `${badge(q.maxScale)}<div class="row-main"><div class="row-title">${esc(
        q.hypocenter?.name || "震源調査中",
      )}</div><div class="row-sub">${esc(q.originTime.slice(5, 16))} ${q.hypocenter?.magnitude != null ? "M" + q.hypocenter.magnitude.toFixed(1) : ""} ・${q.infoLabel}</div></div>`;
    }
    case "eew": {
      const e = latestEew(g);
      return `${badge(e.max_scale)}<div class="row-main"><div class="row-title eew-title">${e.test ? "[テスト] " : ""}緊急地震速報 ${
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
    map.setEpicenter(q.hypocenter?.latitude ?? null, q.hypocenter?.longitude ?? null);
  } else if (g?.kind === "eew") {
    const e = latestEew(g);
    map.setPrefScales(e.cancelled ? [] : e.pref_max, true);
    map.setEpicenter(e.hypocenter?.latitude ?? null, e.hypocenter?.longitude ?? null);
  } else {
    map.setPrefScales([]);
    map.setEpicenter(null, null);
  }
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
  return store.list().find((g) => g.kind === "quake" || g.kind === "eew" || g.kind === "tsunami");
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
        <div class="detail-title">${esc(q.hypocenter?.name || "震源調査中")}</div>
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
        <div><div class="detail-kind eew-title">緊急地震速報 (警報)${e.test ? " [テスト]" : ""} 第${esc(e.serial)}報</div>
        <div class="detail-title">${e.cancelled ? "取り消されました" : esc(e.hypocenter?.name ?? "震源不明")}</div>
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

function activeEew(now: number): EewEvent | null {
  for (const g of store.list()) {
    if (g.kind !== "eew") continue;
    const e = latestEew(g);
    if (!e.cancelled && now - e.received_at_ms < EEW_BANNER_MS) return e;
  }
  return null;
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

/** P波・S波を描く対象 (直近の EEW、なければ直近の地震情報) */
function waveSource(now: number): (Center & { origin: number }) | null {
  for (const g of store.list()) {
    const geo = geoOf(g);
    if (!geo || geo.origin == null) continue;
    if (now - geo.origin > WAVE_MAX_SEC * 1000) return null; // 新しい順なのでこれより前は不要
    if (geo.center) return { ...geo.center, origin: geo.origin };
  }
  return null;
}

interface Scene {
  center: Center | null;
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
    return { center: geo.center, t: ((now - selectedAt) / 1000) * REPLAY_SPEED, shaken: map.prefBox(geo.prefs), replay: true };
  }
  const src = waveSource(now);
  const g = store.list().find((g) => g.kind === "quake" || g.kind === "eew");
  if (!src && (!g || now - g.updatedAt > FULL_MS)) return null;
  const geo = g && geoOf(g);
  return {
    center: src ?? geo?.center ?? null,
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
  map.setWaves(c, surfaceRadiusKm(VP_KM_S, c.depth, sc.t!), s);
  $("#wave-info").textContent = sc.replay ? `再生中 ${sc.t!.toFixed(0)}秒 (×${REPLAY_SPEED})` : `発生から${sc.t!.toFixed(0)}秒`;
  return { box: pad(pointBox(x, y, followRadiusKm(s, stop))), waving };
}

function renderBanner(now: number): void {
  const e = activeEew(now);
  const banner = $("#eew-banner");
  banner.hidden = !e;
  if (!e) return;
  const prefs = e.pref_max.map((p) => p.pref).join("・");
  banner.innerHTML = `<b>${e.test ? "【テスト】" : ""}緊急地震速報 (警報)</b> ${esc(e.hypocenter?.name ?? "")} で地震 ・ 強い揺れに警戒: ${esc(prefs || "—")}`;
}

let raf = 0;
let lastPip = -1;
let timer = 0;
function tick(): void {
  cancelAnimationFrame(raf);
  clearTimeout(timer);
  const now = conn.now();
  const d = new Date(now);
  $("#clock-date").textContent = d.toLocaleDateString("ja-JP", { timeZone: "Asia/Tokyo" });
  $("#clock-time").textContent = d.toLocaleTimeString("ja-JP", { timeZone: "Asia/Tokyo", hour12: false });
  renderBanner(now);
  const sc = scene(now);
  const { box, waving } = renderScene(sc);
  // 波の広がり中 (ライブのみ) は 2 秒ごとに短い音で警戒中を知らせる
  const pip = waving && !sc!.replay ? Math.floor(sc!.t! / 2) : -1;
  if (pip > lastPip) play("pip");
  lastPip = pip;
  if (!waving) {
    map.setWaves(null, null, null);
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
    if (!g || !live) continue;
    const lv = alertLevel(e, g.events.length === 1, activeEew(conn.now()) !== null);
    if (lv && (!alert || rank[lv] > rank[alert])) alert = lv;
  }
  renderList();
  renderDetail();
  if (alert) play(alert);
  tick();
}

function setStatus(s: Status): void {
  const el = $("#status");
  el.dataset.status = s;
  el.title = { connecting: "接続中", open: "接続済み", closed: "切断 (再接続します)" }[s];
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

map
  .load("japan.geojson")
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
