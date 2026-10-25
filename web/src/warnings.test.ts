import { test } from "node:test";
import assert from "node:assert/strict";
import { mergeFocuses, prefOf, topLevel, warnFocusAt, warnFocuses, warningLevel, warningSummary } from "./warnings.ts";

test("warning names map to levels, including ones without a level number", () => {
  assert.equal(warningLevel("レベル２大雨注意報"), "advisory");
  assert.equal(warningLevel("強風注意報"), "advisory");
  assert.equal(warningLevel("レベル３大雨警報"), "warning");
  assert.equal(warningLevel("暴風警報"), "warning");
  assert.equal(warningLevel("レベル４土砂災害危険警報"), "danger");
  assert.equal(warningLevel("レベル５大雨特別警報"), "emergency");
});

test("an area is colored by its highest level", () => {
  const k = (name: string) => ({ code: "", name });
  assert.equal(topLevel([k("雷注意報"), k("レベル３大雨警報"), k("強風注意報")]), "warning");
  assert.equal(topLevel([k("波浪注意報")]), "advisory");
  assert.equal(topLevel([k("レベル３大雨警報"), k("レベル５大雨特別警報")]), "emergency");
});

test("warnings and above are summarized per kind, prefecture and municipality; advisories are left out", () => {
  const k = (name: string) => ({ code: "", name });
  const w = {
    reported_at: "",
    areas: {
      "3310000": [k("レベル３大雨警報"), k("雷注意報")],
      "3320200": [k("レベル３大雨警報")],
      "3420700": [k("レベル３大雨警報")],
      "3420200": [k("レベル５大雨特別警報")],
      "0110000": [k("強風注意報")],
    },
    names: { "3310000": "岡山市", "3320200": "倉敷市", "3420700": "福山市", "3420200": "呉市" },
  };
  assert.equal(prefOf("3310000"), "岡山県");
  assert.deepEqual(warningSummary(w), {
    top: "emergency",
    lines: ["レベル５大雨特別警報: 広島県 呉市", "レベル３大雨警報: 岡山県 岡山市・倉敷市、広島県 福山市"],
  });
  // 1 県の市町村が多いときは「ほか」でまとめる
  assert.deepEqual(warningSummary(w, 1)?.lines[1], "レベル３大雨警報: 岡山県 岡山市 ほか1、広島県 福山市");
  // 注意報だけなら出さない
  assert.equal(warningSummary({ reported_at: "", areas: { "0110000": [k("強風注意報")] } }), null);
});

const kind = (name: string) => ({ code: "", name });

test("only danger and emergency areas are focused, one prefecture at a time, the highest first", () => {
  const w = {
    reported_at: "",
    areas: {
      "3020500": [kind("レベル４土砂災害危険警報"), kind("レベル２大雨注意報")],
      "3036100": [kind("レベル４土砂災害危険警報")],
      "2720000": [kind("レベル３大雨警報")],
      "2920100": [kind("レベル４土砂災害危険警報")],
    },
    names: { "3020500": "有田市", "3036100": "有田川町", "2920100": "奈良市" },
  };
  const fs = warnFocuses(w);
  assert.deepEqual(
    fs.map((f) => [f.prefs, f.codes]),
    [
      [["奈良県"], ["2920100"]],
      [["和歌山県"], ["3020500", "3036100"]],
    ],
  );
  // カードの中身は寄る県の危険警報以上だけ (注意報・ほかの県は入れない)
  assert.deepEqual(fs[1].items, [{ level: "danger", name: "レベル４土砂災害危険警報", places: "和歌山県 有田市・有田川町" }]);
  assert.equal(fs[1].top, "danger");
  assert.deepEqual(warnFocuses({ reported_at: "", areas: { "2720000": [kind("レベル３大雨警報")] } }), []);
  assert.deepEqual(warnFocuses(null), []);
});

test("when a special warning is out, only its prefectures are focused", () => {
  const w = {
    reported_at: "",
    areas: { "3020500": [kind("レベル４土砂災害危険警報")], "4020000": [kind("レベル５大雨特別警報"), kind("レベル４土砂災害危険警報")] },
  };
  assert.deepEqual(
    warnFocuses(w).map((f) => [f.prefs, f.top, f.items.map((i) => i.name)]),
    [[["福岡県"], "emergency", ["レベル５大雨特別警報", "レベル４土砂災害危険警報"]]],
  );
});

test("when danger spreads over many prefectures, the map stays on the whole country", () => {
  const areas = (n: number) => Object.fromEntries(Array.from({ length: n }, (_, i) => [`${String(i + 1).padStart(2, "0")}00000`, [kind("レベル４土砂災害危険警報")]]));
  assert.equal(warnFocuses({ reported_at: "", areas: areas(5) }).length, 5);
  assert.deepEqual(warnFocuses({ reported_at: "", areas: areas(6) }), []);
});

test("neighboring prefectures are shown together when they fit in one view", () => {
  const w = {
    reported_at: "",
    areas: {
      "2920100": [kind("レベル４土砂災害危険警報")],
      "3020500": [kind("レベル４土砂災害危険警報")],
      "0110000": [kind("レベル４土砂災害危険警報")],
    },
    names: { "2920100": "奈良市", "3020500": "有田市", "0110000": "札幌市" },
  };
  const boxes: Record<string, { x0: number; y0: number; x1: number; y1: number }> = {
    "2920100": { x0: 0, y0: 0, x1: 10, y1: 10 },
    "3020500": { x0: 20, y0: 20, x1: 30, y1: 30 },
    "0110000": { x0: 900, y0: -900, x1: 950, y1: -850 },
  };
  const boxOf = (codes: string[]) => codes.map((c) => boxes[c]).reduce((a, b) => ({ x0: Math.min(a.x0, b.x0), y0: Math.min(a.y0, b.y0), x1: Math.max(a.x1, b.x1), y1: Math.max(a.y1, b.y1) }));
  const merged = mergeFocuses(warnFocuses(w), w, boxOf, 100);
  assert.deepEqual(
    merged.map((f) => [f.prefs, f.codes]),
    [
      [["北海道"], ["0110000"]],
      [["奈良県", "和歌山県"], ["2920100", "3020500"]],
    ],
  );
  // 1 種類の文に両方の県が入る
  assert.deepEqual(merged[1].items, [{ level: "danger", name: "レベル４土砂災害危険警報", places: "奈良県 奈良市、和歌山県 有田市" }]);
  // 区域が読めていない (null) ならまとめない
  assert.equal(mergeFocuses(warnFocuses(w), w, () => null, 100).length, 3);
});

test("the map zooms in for the last part of each minute, visiting each focus in turn", () => {
  const f = (pref: string) => ({ prefs: [pref], codes: [], items: [], top: "danger" as const });
  const fs = [f("奈良県"), f("和歌山県")];
  assert.equal(warnFocusAt(fs, 30_000), null);
  assert.deepEqual(warnFocusAt(fs, 50_000)?.prefs, ["奈良県"]);
  assert.deepEqual(warnFocusAt(fs, 60_000 + 50_000)?.prefs, ["和歌山県"]);
  assert.deepEqual(warnFocusAt(fs, 120_000 + 50_000)?.prefs, ["奈良県"]);
  assert.equal(warnFocusAt([], 50_000), null);
});
