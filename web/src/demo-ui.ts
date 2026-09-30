// デモモード: 場面の一覧と再生・操作 (ブラウザの中だけで再生し、実際の情報は裏で受け続ける)。

import { type Scenario, type ScenarioSummary, makePlan } from "./demo.ts";
import { hindsightOf, historyStart, skipRanges } from "./history.ts";
import { GroupStore } from "./groups.ts";
import { esc } from "./html.ts";
import type { EqEvent } from "./types.ts";
import { $, map } from "./dom.ts";
import { type DemoState, app, demoPos, hooks, liveWorld, now, type World } from "./state.ts";

/** 倍速の選択肢 */
const SPEEDS = [1, 2, 4, 8];
/** 最後の報の後も、揺れの広がりが終わってカメラが戻るまで再生する */
const TAIL_MS = 4 * 60_000;

/** 時計を飛ばしたあと、「早送り」を出し続ける時間 */
const FF_SHOW_MS = 3000;

/** 再生の長さ (最後の報 + TAIL_MS) */
const lengthOf = (d: DemoState) => (d.plan ? d.plan.end + TAIL_MS : 0);

function showWorld(w: World): void {
  app.world = w;
  app.numbers = new Map();
  app.selectedKey = null;
  app.tourStart = null;
  app.tourHold = null;
  map.release();
}

export async function enterDemo(): Promise<void> {
  if (app.demo) return;
  const res = await fetch("demo/index.json");
  const scenarios: ScenarioSummary[] = res.ok ? await res.json() : [];
  app.demo = { scenarios, running: null, plan: null, world: null, applied: 0, run: 0, history: false, skips: [], ffUntil: 0, hindsight: null, clock: { pos: 0, anchor: performance.now(), speed: 1, paused: false } };
  showWorld({ store: new GroupStore(), tsunami: null, userquake: null });
  hooks.renderAll();
}

export function exitDemo(): void {
  if (!app.demo) return;
  app.demo = null;
  showWorld(liveWorld);
  hooks.renderAll();
}

export async function runScenario(id: string): Promise<void> {
  if (!app.demo) await enterDemo();
  const d = app.demo;
  if (!d) return;
  const res = await fetch(`demo/${encodeURIComponent(id)}.json`);
  if (!res.ok || app.demo !== d) return;
  const scenario: Scenario = await res.json();
  // 記録の場面は当時の日時のまま、架空の場面は「今」から始まるようにずらす
  d.plan = null;
  const startAt = scenario.source ? undefined : now();
  d.plan = makePlan(scenario.events, ++d.run, startAt);
  d.running = id;
  d.clock = { pos: 0, anchor: performance.now(), speed: d.clock.speed, paused: false };
  rebuild(d);
  hooks.renderAll();
}

/**
 * 履歴の再生を始める: 過去の地震の報を、当時の時刻で ×1 から流す (最初の揺れの HISTORY_LEAD_MS 前から)。
 * 始まりの時刻が分からなければ何もせず false を返す
 */
export function startHistory(events: EqEvent[]): boolean {
  const start = historyStart(events);
  if (start == null) return false;
  const run = (app.demo?.run ?? 0) + 1;
  const plan = makePlan(events, run, undefined, start);
  app.demo = { scenarios: [], running: null, plan, world: null, applied: 0, run, history: true, skips: skipRanges(plan.events.map((x) => x.at)), ffUntil: 0, hindsight: hindsightOf(events), clock: { pos: 0, anchor: performance.now(), speed: 1, paused: false } };
  rebuild(app.demo);
  hooks.renderAll();
  return true;
}

/** 場面の世界をまっさらにする (最初から、または巻き戻したとき) */
function rebuild(d: DemoState): void {
  d.world = { store: new GroupStore(), tsunami: null, userquake: null };
  d.applied = 0;
  showWorld(d.world);
}

/**
 * 再生位置までの情報を流す。quiet なら警戒音などを鳴らさない (位置を動かしたとき)。
 * 画面の更新 (tick) ごとに呼ぶ
 */
export function advanceDemo(quiet = false): void {
  const d = app.demo;
  if (!d?.plan || !d.world || app.world !== d.world) return;
  let pos = demoPos(d);
  // 何も届かない長い間は、次の報の少し前まで時計を飛ばす
  const skip = d.skips.find((x) => pos >= x.from && pos < x.to);
  if (skip) {
    d.clock = { ...d.clock, pos: skip.to, anchor: performance.now() };
    d.ffUntil = performance.now() + FF_SHOW_MS;
    pos = skip.to;
  }
  const due = [];
  while (d.applied < d.plan.events.length && d.plan.events[d.applied].at <= pos) due.push(d.plan.events[d.applied++].event);
  if (due.length) hooks.onEvents(due, !quiet, d.world);
}

/** 再生位置を動かす。戻すときは最初から流し直す */
function seek(d: DemoState, pos: number): void {
  if (pos < demoPos(d)) rebuild(d);
  d.clock = { ...d.clock, pos, anchor: performance.now() };
  advanceDemo(true);
  hooks.renderAll();
}

function setClock(d: DemoState, change: Partial<DemoState["clock"]>): void {
  d.clock = { ...d.clock, pos: demoPos(d), anchor: performance.now(), ...change };
  renderDemoPanel();
}

const mmss = (ms: number) => {
  const s = Math.max(0, Math.round(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
};

export function renderDemoPanel(): void {
  const panel = $("#demo-panel");
  panel.hidden = !app.demo;
  if (!app.demo) return;
  const d = app.demo;
  $("#demo-head").hidden = d.history;
  $("#history-head").hidden = !d.history;
  $("#demo-list").hidden = d.history;
  const html = d.scenarios
    .map(
      (s) => `<li class="${s.id === d.running ? "running" : ""}"><div class="demo-text"><b>${esc(s.name)}</b><div class="muted">${esc(
        s.description,
      )}</div>${s.source ? `<div class="muted">出典: ${esc(s.source)}</div>` : ""}</div><button type="button" class="follow" data-id="${esc(s.id)}">${s.id === d.running ? "最初から" : "実行"}</button></li>`,
    )
    .join("");
  const list = $("#demo-list");
  if (list.innerHTML !== html) list.innerHTML = html;
  renderDemoControls();
}

/** 再生の操作 (一時停止・倍速・位置)。位置と時間の表示は tick ごとに直す */
export function renderDemoControls(): void {
  const box = $("#demo-controls");
  const d = app.demo;
  box.hidden = !d?.plan;
  if (!d?.plan) return;
  const max = lengthOf(d);
  // 末尾まで来たら、デモは止め、履歴の再生はライブに戻る (描き直しの最中に画面を組み替えないよう、あとで)
  if (!d.clock.paused && demoPos(d) >= max) {
    if (d.history) return void setTimeout(exitDemo, 0);
    setClock(d, { paused: true, pos: max });
  }
  const pos = Math.min(demoPos(d), max);
  const play = $<HTMLButtonElement>("#demo-play");
  const label = d.clock.paused ? "再生" : "一時停止";
  if (play.textContent !== label) play.textContent = label;
  for (const b of box.querySelectorAll<HTMLButtonElement>("button[data-speed]")) b.classList.toggle("active", Number(b.dataset.speed) === d.clock.speed);
  const range = $<HTMLInputElement>("#demo-seek");
  range.max = String(max);
  // 利用者が動かしている最中は上書きしない
  if (document.activeElement !== range) range.value = String(pos);
  const time = `${mmss(pos)} / ${mmss(max)}`;
  const t = $("#demo-time");
  if (t.textContent !== time) t.textContent = time;
  const ff = performance.now() < d.ffUntil ? "早送り" : "";
  const f = $("#demo-ff");
  if (f.textContent !== ff) f.textContent = ff;
}

$("#demo-open").addEventListener("click", () => void enterDemo());

$("#demo-list").addEventListener("click", (e) => {
  const id = (e.target as HTMLElement).closest<HTMLElement>("button[data-id]")?.dataset.id;
  if (id) void runScenario(id);
});

$("#demo-controls").innerHTML = `<button type="button" id="demo-play" class="follow">一時停止</button>${SPEEDS.map(
  (s) => `<button type="button" class="follow speed" data-speed="${s}">×${s}</button>`,
).join("")}<input type="range" id="demo-seek" min="0" step="1000" aria-label="再生位置"><span id="demo-time" class="muted"></span><span id="demo-ff" class="muted"></span>`;

$("#demo-controls").addEventListener("click", (e) => {
  const d = app.demo;
  const b = (e.target as HTMLElement).closest<HTMLButtonElement>("button");
  if (!d?.plan || !b) return;
  // 末尾で止まっているときの「再生」は最初から
  if (b.id === "demo-play" && d.clock.paused && demoPos(d) >= lengthOf(d)) {
    seek(d, 0);
    setClock(d, { paused: false });
  } else if (b.id === "demo-play") setClock(d, { paused: !d.clock.paused });
  else if (b.dataset.speed) setClock(d, { speed: Number(b.dataset.speed) });
});

$<HTMLInputElement>("#demo-seek").addEventListener("input", (e) => {
  const d = app.demo;
  if (d?.plan) seek(d, Number((e.target as HTMLInputElement).value));
});
