// 起動、情報の受信、周期的な描画 (tick)。

// 配信用の表示の設定は、ほかのモジュールが設定を読む前に済ませる
import "./broadcast.ts";
import { type AlertLevel, alertLevel } from "./alert.ts";
import { loadTelop, renderClock, renderSound, renderTelop, setStatus } from "./chrome.ts";
import { Connection } from "./connection.ts";
import { advanceDemo, enterDemo, exitDemo, fastForwarding, renderDemoControls, renderDemoPanel, runScenario, startHistory } from "./demo-ui.ts";
import { fadeOpacity } from "./fade.ts";
import { fetchArchive, gatherEvents } from "./history.ts";
import { esc } from "./html.ts";
import { notify, renderCountdown, updateHome } from "./personal-ui.ts";
import { sameQuake } from "./priority.ts";
import { activeEews, calmState, currentGroup, displayedInfoMs, placeOf, priorityGroups, relatedQuake, updateNumbers, updateTour } from "./quakes.ts";
import { renderMarkers, renderScene, scene } from "./scene.ts";
import { pipSlot } from "./replay-sound.ts";
import { play, soundEnabled } from "./sound.ts";
import { $, map } from "./dom.ts";
import { type World, app, hooks, liveWorld, now, now as serverNow } from "./state.ts";
import { activeAreas, latestTsunami, tsunamiAlert } from "./tsunami.ts";
import type { EewEvent, EqEvent, UserquakeEvent } from "./types.ts";
import { confidenceGrade, latestUserquake, userquakeShown } from "./userquake.ts";
import { type Warnings, topLevel, warningSummary } from "./warnings.ts";
import { notifyVoice, mixerAudio } from "./broadcast.ts";
import { enqueueVoice, voiceUrl } from "./voice.ts";
import { loadBgmConfig, updateBgm } from "./bgm.ts";
import { loadBanners, updateBanner } from "./banner.ts";
import { loadCityWeather, renderCityWeather } from "./weather-layer.ts";
import { numTag, renderBanner, renderDetail, renderList, renderMode, renderTsunamiBanner, updatePointsOpen } from "./view.ts";
import { latestEew, summarizeQuake } from "./groups.ts";

/** この時間内に続けて届いた情報では、前より強い音のときだけ鳴らす */
const ALERT_MERGE_MS = 3000;

let raf = 0;

let lastPip = -1;

let lastCurrentKey: string | undefined;

/** 直前に鳴らした警戒音 (数秒以内に重なったら強い方だけ鳴らす) */
let lastAlert = { level: "info" as AlertLevel, at: 0 };

const RANK: Record<AlertLevel, number> = { info: 0, low: 1, medium: 2, strong: 3 };

/** 警戒音を鳴らす。数秒以内に重なったら、前より強いときだけ鳴らす */
function playAlert(level: AlertLevel): void {
  if (now() - lastAlert.at < ALERT_MERGE_MS && RANK[level] <= RANK[lastAlert.level]) return;
  play(level);
  lastAlert = { level, at: now() };
}

/** 音声読み上げ (設定が ON のとき)。mixer ならサーバへ URL を知らせ、通常はページで鳴らす */
function speak(id: string, group: string): void {
  // 画面右上の「音」が OFF なら、警戒音と同じく読み上げも止める
  if (!app.settings.voice || !soundEnabled()) return;
  if (mixerAudio) notifyVoice(voiceUrl(id, location.href));
  else enqueueVoice(id, group);
}

let timer = 0;

export function tick(): void {
  cancelAnimationFrame(raf);
  clearTimeout(timer);
  advanceDemo();
  const now = serverNow();
  renderClock(now);
  if (updateNumbers(now)) {
    renderList();
    renderDetail();
  }
  const prevTour = app.tourKey;
  updateTour(now, map.userMoved);
  if (app.tourKey && app.tourKey !== prevTour) showTourToast(app.tourKey);
  renderBanner(now);
  renderMarkers(now);
  const sc = scene(now);
  const { box, waving } = renderScene(sc);
  // 波の広がり中 (履歴の再生の早送りの最中を除く) は 2 秒ごとに短い音で警戒中を知らせる (地震が重なっても 1 本)
  const pip = pipSlot(now, { waving, replay: sc?.replay ?? false, speed: app.demo?.clock.speed ?? 1, fastForward: fastForwarding() });
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
  renderTelop(now, waving || activeEews(now).length > 0 || priorityGroups(now).length > 0 || activeAreas(app.world.tsunami).length > 0 || app.demo != null);
  if (!waving) {
    map.setWaves([]);
    $("#wave-info").textContent = "";
  }
  map.setTarget(box);
  // 平時は地震の塗り分けを消して気象警報・注意報を塗る。BGM は地震・津波・揺れの報告の間とデモ中は止める
  const { calm, quiet } = renderWarnings(now, renderUserquake(now));
  updateBgm(quiet);
  updateBanner(quiet);
  renderCityWeather(calm, now);
  map.setFade(app.selectedKey ? 1 : calm ? 0 : fadeOpacity(now - displayedInfoMs()));
  renderMode();
  renderSound();
  renderCountdown(now);
  updatePointsOpen();
  renderDemoControls();
  // 波の表示中は滑らかに、そうでなければ時計の更新だけ
  if (waving) raf = requestAnimationFrame(tick);
  // デモの再生中は倍速でも情報が遅れないよう細かく
  else timer = window.setTimeout(tick, app.demo?.plan && !app.demo.clock.paused ? 200 : 1000);
}

/** 実際の情報のうち、デモモードを直ちに終えて見せるべきもの */
function urgent(e: EqEvent): boolean {
  if (e.kind === "eew") return !e.test && !e.cancelled;
  if (e.kind === "quake") return e.max_scale >= 30;
  return e.kind === "tsunami" && !e.cancelled && e.areas.length > 0;
}

/** target は情報を入れる先。デモモード中も実際の情報は liveWorld に入れ続ける */
export function onEvents(all: EqEvent[], live: boolean, target: World = liveWorld): void {
  let alert: AlertLevel | null = null;
  // 地震感知情報は一覧に入れず、最新のものだけ持つ (地図の印は tick で描く)
  for (const e of all) if (e.kind === "userquake") receiveUserquake(e, live, target);
  const events = all.filter((e) => e.kind !== "userquake");
  if (!events.length) return;
  // デモを見ている間に届いた実際の情報: 裏で蓄えるだけ。大事な情報ならデモを終えて表示する
  if (target !== app.world) {
    for (const e of events) {
      if (!target.store.add(e)) continue;
      if (e.kind === "tsunami") target.tsunami = latestTsunami(target.tsunami, e);
    }
    if (live && events.some(urgent)) exitDemo();
    return;
  }
  for (const e of events) {
    const g = app.world.store.add(e);
    if (!g) continue;
    if (e.kind === "tsunami") {
      const prev = activeAreas(app.world.tsunami);
      app.world.tsunami = latestTsunami(app.world.tsunami, e);
      const lv = live ? tsunamiAlert(prev, activeAreas(app.world.tsunami)) : null;
      if (lv && (!alert || RANK[lv] > RANK[alert])) alert = lv;
      if (lv) speak(e.id, "tsunami");
    }
    if (!live) continue;
    // その地震の EEW で既に鳴らしていれば、地震情報では鳴らさない
    const eewActive =
      e.kind === "quake" && activeEews(now()).some((x) => sameQuake(placeOf(x.origin_time_ms, x.hypocenter), placeOf(e.origin_time_ms, e.hypocenter)));
    // EEW は予報から警報に上がったときも鳴らす
    const isNew =
      g.events.length === 1 || (e.kind === "eew" && e.warning && !g.events.slice(0, -1).some((x) => (x as EewEvent).warning));
    // 同じ地震のそれまでの EEW の最大震度 (最初でない EEW が震度を上げたかの判断)
    const prevMax = Math.max(-1, ...g.events.slice(0, -1).map((x) => (x as EewEvent).max_scale));
    const lv = alertLevel(e, isNew, eewActive, prevMax);
    if (lv && (!alert || RANK[lv] > RANK[alert])) alert = lv;
    if (lv) speak(e.id, g.key);
    // 新しい地震は、巡回より先にしばらく見せる
    if (isNew && (e.kind === "eew" || e.kind === "quake")) {
      app.tourHold = { key: g.key, until: now() + Math.max(20, app.settings.tourSec * 2) * 1000 };
    }
  }
  // Wolfx 経由の情報を受けたら出典を出す
  if (events.some((e) => e.source === "wolfx")) $("#credit-wolfx").hidden = false;
  // ブラウザ通知は実際の情報だけ (デモは通知しない)
  if (live && target === liveWorld) events.forEach(notify);
  renderAll();
  if (alert) playAlert(alert);
}

let toastTimer = 0;
/** 巡回で次の地震へ移ったとき、番号と名前を短く出す */
function receiveUserquake(e: UserquakeEvent, live: boolean, target: World): void {
  const prev = target.userquake;
  target.userquake = latestUserquake(prev, e);
  // 新しい揺れの報告が始まったときだけ知らせる (同じ揺れの評価の更新では鳴らさない)
  if (!live || target !== app.world || prev?.started_at === e.started_at) return;
  const names = [...e.areas]
    .sort((a, b) => b.count - a.count)
    .map((a) => app.userquakeAreas.get(a.code)?.[0])
    .filter((n): n is string => !!n);
  play("feel");
  showToast(`揺れの報告: ${esc(names.slice(0, 3).join("、") || "地域不明")}${names.length > 3 ? " ほか" : ""}`);
}

/** 地震感知情報の印を地図に出す (出す間だけ)。出したかを返す */
function renderUserquake(now: number): boolean {
  const u = app.world.userquake;
  const official = app.world.store
    .list()
    .filter((g) => g.kind === "eew" || g.kind === "quake")
    .map((g) => g.updatedAt);
  if (!u || !userquakeShown(u, now, official)) {
    map.setUserquake([]);
    return false;
  }
  map.setUserquake(
    u.areas.flatMap((a) => {
      const p = app.userquakeAreas.get(a.code);
      return p ? [{ name: p[0], lat: p[1], lon: p[2], count: a.count, grade: confidenceGrade(a.confidence) }] : [];
    }),
  );
  return true;
}

/**
 * 平時 (地震・津波・揺れの報告の表示が無く、実際の情報を見ているとき) だけ気象警報・注意報を塗る。
 * 地震の情報が届けば地震の表示に切り替わり、落ち着けば平時に戻る
 */
function renderWarnings(now: number, feeling: boolean): { calm: boolean; quiet: boolean } {
  const { quiet: still, calm, tsunami } = calmState(now, feeling);
  // 地震の表示の間は、すぐ平時に戻すボタン (津波予報が出ている間とデモ中は出さない)
  $("#calm-now").hidden = calm || tsunami || app.demo != null;
  const w = calm ? app.warnings : null;
  const items = w
    ? Object.entries(w.areas).map(([code, kinds]) => ({ code, level: topLevel(kinds), text: kinds.map((k) => k.name).join("、") }))
    : [];
  if (items.length && !map.warningAreasLoaded && !warningAreasLoading) {
    warningAreasLoading = true;
    map.loadWarningAreas("warning-areas.geojson").catch(() => (warningAreasLoading = false));
  }
  map.setWarnings(items);
  $("#legend-warn").hidden = items.length === 0;
  renderWarnBanner(w);
  return { calm, quiet: still };
}
let warningAreasLoading = false;

/** 警報以上を文字で知らせる (平時だけ。注意報は地図の色とツールチップだけ) */
let warnSig = "";
function renderWarnBanner(w: Warnings | null): void {
  const box = $("#warn-banner");
  const summary = w ? warningSummary(w) : null;
  box.hidden = !summary;
  if (!summary) return;
  // 幅が変わったときも流すかを判定し直す
  const sig = JSON.stringify([summary, box.clientWidth]);
  if (sig === warnSig) return;
  warnSig = sig;
  box.dataset.level = summary.top;
  const text = box.querySelector<HTMLElement>(".warn-text")!;
  text.textContent = `【気象警報】 ${summary.lines.join(" ／ ")}`;
  // 収まらないときは横に流す (長さに合わせて速さをそろえる)
  box.classList.remove("scroll");
  if (text.scrollWidth > box.clientWidth) {
    box.style.setProperty("--warn-sec", `${Math.max(20, Math.round(text.textContent.length / 4))}s`);
    box.classList.add("scroll");
  }
}

/** 発表中の気象警報・注意報を取り直す (サーバは 5 分ごとに気象庁から取得している) */
async function loadWarnings(): Promise<void> {
  const res = await fetch("api/warnings");
  if (res.ok) app.warnings = await res.json();
}

async function loadUserquakeAreas(): Promise<void> {
  const res = await fetch("userquake-areas.json");
  if (!res.ok) return;
  const rows: Record<string, [string, number, number]> = await res.json();
  app.userquakeAreas = new Map(Object.entries(rows).map(([code, v]) => [Number(code), v]));
}

function showTourToast(key: string): void {
  const g = app.world.store.get(key);
  const name = g && (g.kind === "quake" ? summarizeQuake(g).hypocenter?.name : g.kind === "eew" ? latestEew(g).hypocenter?.name : "");
  showToast(`${numTag(key)}${esc(name || "震源調査中")}`);
}

/** 画面上部に短く知らせる (html はエスケープ済みのもの) */
function showToast(html: string): void {
  const el = $("#tour-toast");
  el.innerHTML = html;
  el.hidden = false;
  el.classList.remove("show");
  void el.offsetWidth;
  el.classList.add("show");
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => (el.hidden = true), 2500);
}

/** 表示中のデータで画面全体を描き直す */
export function renderAll(): void {
  renderList();
  renderDetail();
  map.setTsunami(activeAreas(app.world.tsunami));
  $("#legend-tsunami").hidden = activeAreas(app.world.tsunami).length === 0;
  renderTsunamiBanner();
  renderDemoPanel();
  tick();
}

async function loadStations(): Promise<void> {
  const res = await fetch("stations.json");
  if (!res.ok) return;
  const rows: [string, number, number, string][] = await res.json();
  app.stations = new Map(rows.map(([name, lat, lon, area]) => [name, { lat, lon, area }]));
}

/**
 * 履歴の行を選ぶ。地震 (発生時刻が分かるもの) は、その報を集めて当時の時刻で再生する。
 * 再生中は行を選べない (「リアルタイムに戻る」で戻ってから)。デモ中 (と発生時刻が分からないとき) は、選んだ時点を発生とみなして波だけ描く
 */
async function select(key: string): Promise<void> {
  if (app.demo?.history) return;
  // 津波予報の行は、その地震 (直前の地震情報・緊急地震速報) を再生する
  const g = app.world.store.get(key);
  const quake = g && relatedQuake(g);
  if (!app.demo && quake && startHistory(await gatherEvents(quake, fetchArchive))) return;
  app.selectedKey = key;
  app.selectedAt = now();
  map.release();
  renderList();
  renderDetail();
}

$("#list").addEventListener("click", (e) => {
  const key = (e.target as HTMLElement).closest("li")?.dataset.key;
  if (key) void select(key);
});

// 画面外の地震の矢印からも選べる
map.onSelect = (key) => void select(key);

// 全体図: 日本全体を表示する。震央を押すとその地震へ寄る (「リアルタイムに戻る」で自動に戻る)
// 警報・注意報: 今の地震の表示を終えて平時に戻す (次に新しい地震の情報が届けば、また地震の表示になる)
$("#calm-now").addEventListener("click", () => {
  app.calmSince = now();
  app.selectedKey = null;
  map.release();
  renderAll();
});
$("#overview").addEventListener("click", () => {
  map.showOverview();
  renderMode();
});
// 観測点の一覧を自分で開閉したら、その地震は自動で畳まない
$("#detail").addEventListener("click", (e) => {
  const summary = (e.target as HTMLElement).closest("summary");
  const box = summary?.parentElement as HTMLDetailsElement | undefined;
  if (box?.dataset.key) app.listOpen.set(box.dataset.key, !box.open);
});
$("#back-live").addEventListener("click", () => {
  if (app.demo && !app.selectedKey && !map.userMoved) exitDemo();
  else {
    app.selectedKey = null;
    map.release();
    renderAll();
  }
});

// ほかのモジュールから呼ぶ処理を登録する
hooks.renderAll = renderAll;
hooks.onEvents = onEvents;
hooks.playAlert = playAlert;

loadTelop();
// 気象警報・注意報は 5 分ごとに取り直す
window.setInterval(() => void loadWarnings().catch(() => {}), 5 * 60_000);
// 主要都市の天気とアメダスの雨も 5 分ごと (サーバは 10 分ごとに取得)
window.setInterval(() => void loadCityWeather().catch(() => {}), 5 * 60_000);

Promise.all([
  map.load("japan.geojson"),
  // 無くても地震の表示はできる
  map.loadTsunami("tsunami.geojson").catch(() => {}),
  // 無ければ寄っても都道府県で塗る
  map.loadAreas("areas.geojson").catch(() => {}),
  map.loadNeighbors("neighbors.geojson").catch(() => {}),
  loadStations().catch(() => {}),
  loadUserquakeAreas().catch(() => {}),
  loadWarnings().catch(() => {}),
  loadCityWeather().catch(() => {}),
  loadBgmConfig().catch(() => {}),
  loadBanners().catch(() => {}),
])
  .catch((err) => {
    $("#detail").innerHTML = `<p class="error">${esc(String(err))}</p>`;
  })
  .finally(() => {
    app.conn = new Connection(Connection.defaultUrl(), {
      onSnapshot: (events) => onEvents(events, false),
      onEvent: (event) => onEvents([event], true),
      onStatus: setStatus,
    });
    updateHome();
    app.conn.start();
    tick();
    // ?demo=場面の名前 で開いたらデモを再生する (見守りモニタの動作確認用)
    const demoId = new URLSearchParams(location.search).get("demo");
    if (demoId != null) void (demoId ? runScenario(demoId) : enterDemo());
  });
