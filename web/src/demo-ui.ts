// デモモード: 場面の一覧と再生 (ブラウザの中だけで再生し、実際の情報は裏で受け続ける)。

import { type Scenario, type ScenarioSummary, schedule } from "./demo.ts";
import { GroupStore } from "./groups.ts";
import { esc } from "./html.ts";
import { $, app, hooks, liveWorld, map, now, type World } from "./state.ts";

/** デモモードの状態。null ならデモモードではない */

export function showWorld(w: World): void {
  app.world = w;
  app.numbers = new Map();
  app.selectedKey = null;
  map.release();
}

export async function enterDemo(): Promise<void> {
  if (app.demo) return;
  const res = await fetch("demo/index.json");
  const scenarios: ScenarioSummary[] = res.ok ? await res.json() : [];
  app.demo = { scenarios, running: null, timers: [], run: 0 };
  showWorld({ store: new GroupStore(), tsunami: null });
  hooks.renderAll();
}

export function exitDemo(): void {
  if (!app.demo) return;
  app.demo.timers.forEach(clearTimeout);
  app.demo = null;
  showWorld(liveWorld);
  hooks.renderAll();
}

export async function runScenario(id: string): Promise<void> {
  if (!app.demo) await enterDemo();
  const d = app.demo;
  if (!d) return;
  d.timers.forEach(clearTimeout);
  const res = await fetch(`demo/${encodeURIComponent(id)}.json`);
  if (!res.ok || app.demo !== d) return;
  const scenario: Scenario = await res.json();
  // 場面ごとにまっさらな状態から再生する
  const w: World = { store: new GroupStore(), tsunami: null };
  showWorld(w);
  d.running = id;
  const plan = schedule(scenario.events, now(), ++d.run);
  d.timers = plan.map(({ at, event }) =>
    window.setTimeout(() => {
      if (app.world === w) hooks.onEvents([event], true, w);
    }, at),
  );
  // 最後の情報が届いたら一覧を「再生済み」に戻す
  const last = plan.length ? plan[plan.length - 1].at : 0;
  d.timers.push(
    window.setTimeout(() => {
      if (app.demo === d && app.world === w) {
        d.running = null;
        renderDemoPanel();
      }
    }, last + 1000),
  );
  hooks.renderAll();
}

export function renderDemoPanel(): void {
  const panel = $("#demo-panel");
  panel.hidden = !app.demo;
  if (!app.demo) return;
  const d = app.demo;
  const html = d.scenarios
    .map(
      (s) => `<li class="${s.id === d.running ? "running" : ""}"><div class="demo-text"><b>${esc(s.name)}</b><div class="muted">${esc(
        s.description,
      )}</div></div><button type="button" class="follow" data-id="${esc(s.id)}">${s.id === d.running ? "再生中" : "実行"}</button></li>`,
    )
    .join("");
  const list = $("#demo-list");
  if (list.innerHTML !== html) list.innerHTML = html;
}

$("#demo-open").addEventListener("click", () => void enterDemo());

$("#demo-list").addEventListener("click", (e) => {
  const id = (e.target as HTMLElement).closest<HTMLElement>("button[data-id]")?.dataset.id;
  if (id) void runScenario(id);
});
