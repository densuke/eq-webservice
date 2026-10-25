import { test } from "node:test";
import assert from "node:assert/strict";
import { placeCard, warnCardHtml } from "./warn-card.ts";

const overlap = (a: { x: number; y: number; w: number; h: number }, r: { x0: number; y0: number; x1: number; y1: number }) =>
  Math.max(0, Math.min(a.x + a.w, r.x1) - Math.max(a.x, r.x0)) * Math.max(0, Math.min(a.y + a.h, r.y1) - Math.max(a.y, r.y0));

test("the card goes beside the area without covering it, and the arrow points at the area", () => {
  const region = { x0: 400, y0: 250, x1: 500, y1: 330 };
  const p = placeCard(region, { w: 260, h: 70 }, { w: 900, h: 600 });
  assert.equal(overlap({ x: p.x, y: p.y, w: 260, h: 70 }, region), 0);
  // 画面の中に収まる
  assert.ok(p.x >= 0 && p.y >= 0 && p.x + 260 <= 900 && p.y + 70 <= 600);
  // 矢印は札の縁から区域の中心へ向かう
  assert.ok(Math.hypot(p.toX - 450, p.toY - 290) < Math.hypot(p.fromX - 450, p.fromY - 290));
});

test("on a narrow screen the card still stays on screen", () => {
  const p = placeCard({ x0: 100, y0: 300, x1: 280, y1: 420 }, { w: 300, h: 70 }, { w: 375, h: 700 });
  assert.ok(p.x >= 0 && p.x + 300 <= 375 && p.y >= 0 && p.y + 70 <= 700);
  assert.equal(overlap({ x: p.x, y: p.y, w: 300, h: 70 }, { x0: 100, y0: 300, x1: 280, y1: 420 }), 0);
});

test("the card lists up to two kinds and counts the rest", () => {
  const item = (name: string) => ({ level: "danger" as const, name, places: "和歌山県 有田市" });
  const html = warnCardHtml({ prefs: ["和歌山県"], codes: [], top: "danger", items: [item("A"), item("B"), item("C"), item("D")] });
  assert.match(html, /A/);
  assert.match(html, /B/);
  assert.doesNotMatch(html, />C</);
  assert.match(html, /ほか 2 件/);
  // 文字は HTML として解釈しない
  assert.doesNotMatch(warnCardHtml({ prefs: [], codes: [], top: "danger", items: [item("<b>")] }), /<b>/);
});
