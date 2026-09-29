// 起動、情報の受信、周期的な描画 (tick)。

import { type AlertLevel, alertLevel } from "./alert.ts";
import { loadTelop, renderClock, renderSound, renderTelop, setStatus } from "./chrome.ts";
import { Connection } from "./connection.ts";
import { advanceDemo, enterDemo, exitDemo, renderDemoControls, renderDemoPanel, runScenario } from "./demo-ui.ts";
import { fadeOpacity } from "./fade.ts";
import { esc } from "./html.ts";
import { notify, renderCountdown, updateHome } from "./personal-ui.ts";
import { sameQuake } from "./priority.ts";
import { activeEews, currentGroup, displayedOriginMs, placeOf, priorityGroups, updateNumbers, updateTour } from "./quakes.ts";
import { renderMarkers, renderScene, scene } from "./scene.ts";
import { play } from "./sound.ts";
import { $, map } from "./dom.ts";
import { type World, app, hooks, liveWorld, now, now as serverNow } from "./state.ts";
import { activeAreas, latestTsunami, tsunamiAlert } from "./tsunami.ts";
import type { EewEvent, EqEvent } from "./types.ts";
import { numTag, renderBanner, renderDetail, renderList, renderMode, renderTsunamiBanner, updatePointsOpen } from "./view.ts";
import { latestEew, summarizeQuake } from "./groups.ts";

/** この時間内に続けて届いた情報では、前より強い音のときだけ鳴らす */
export const ALERT_MERGE_MS = 3000;

export let raf = 0;

export let lastPip = -1;

export let lastCurrentKey: string | undefined;

/** 直前に鳴らした警戒音 (数秒以内に重なったら強い方だけ鳴らす) */
export let lastAlert = { level: "info" as AlertLevel, at: 0 };

export let timer = 0;

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
  // 波の広がり中 (ライブのみ) は 2 秒ごとに短い音で警戒中を知らせる (地震が重なっても 1 本)
  const pip = waving && !sc!.replay && (app.demo?.clock.speed ?? 1) <= 1 ? Math.floor(now / 2000) : -1;
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
  map.setFade(app.selectedKey ? 1 : fadeOpacity(now - displayedOriginMs()));
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
export function urgent(e: EqEvent): boolean {
  if (e.kind === "eew") return !e.test && !e.cancelled;
  if (e.kind === "quake") return e.max_scale >= 30;
  return e.kind === "tsunami" && !e.cancelled && e.areas.length > 0;
}

/** target は情報を入れる先。デモモード中も実際の情報は liveWorld に入れ続ける */
export function onEvents(events: EqEvent[], live: boolean, target: World = liveWorld): void {
  const rank: Record<AlertLevel, number> = { info: 0, low: 1, medium: 2, strong: 3 };
  let alert: AlertLevel | null = null;
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
  if (alert && !(now() - lastAlert.at < ALERT_MERGE_MS && rank[alert] <= rank[lastAlert.level])) {
    play(alert);
    lastAlert = { level: alert, at: now() };
  }
}

let toastTimer = 0;
/** 巡回で次の地震へ移ったとき、番号と名前を短く出す */
function showTourToast(key: string): void {
  const g = app.world.store.get(key);
  const name = g && (g.kind === "quake" ? summarizeQuake(g).hypocenter?.name : g.kind === "eew" ? latestEew(g).hypocenter?.name : "");
  const el = $("#tour-toast");
  el.innerHTML = `${numTag(key)}${esc(name || "震源調査中")}`;
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

export async function loadStations(): Promise<void> {
  const res = await fetch("stations.json");
  if (!res.ok) return;
  const rows: [string, number, number, string][] = await res.json();
  app.stations = new Map(rows.map(([name, lat, lon, area]) => [name, { lat, lon, area }]));
}

export function select(key: string): void {
  app.selectedKey = key;
  app.selectedAt = now();
  map.release();
  renderList();
  renderDetail();
}

$("#list").addEventListener("click", (e) => {
  const key = (e.target as HTMLElement).closest("li")?.dataset.key;
  if (key) select(key);
});

// 画面外の地震の矢印からも選べる
map.onSelect = select;

// 全体図: 日本全体を表示する。震央を押すとその地震へ寄る (「リアルタイムに戻る」で自動に戻る)
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

loadTelop();

Promise.all([
  map.load("japan.geojson"),
  // 無くても地震の表示はできる
  map.loadTsunami("tsunami.geojson").catch(() => {}),
  // 無ければ寄っても都道府県で塗る
  map.loadAreas("areas.geojson").catch(() => {}),
  map.loadNeighbors("neighbors.geojson").catch(() => {}),
  loadStations().catch(() => {}),
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
