import { test } from "node:test";
import assert from "node:assert/strict";
import { prefOf, topLevel, warnFocusAt, warnFocuses, warningLevel, warningSummary } from "./warnings.ts";

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

test("only danger and emergency areas are focused, one prefecture at a time, the highest first", () => {
  const k = (name: string) => ({ code: "", name });
  const w = {
    reported_at: "",
    areas: {
      "3020500": [k("レベル４土砂災害危険警報"), k("レベル２大雨注意報")],
      "3036100": [k("レベル４土砂災害危険警報")],
      "2720000": [k("レベル３大雨警報")],
      "2920100": [k("レベル５大雨特別警報")],
    },
    names: { "3020500": "有田市", "3036100": "有田川町", "2920100": "奈良市" },
  };
  const fs = warnFocuses(w);
  assert.deepEqual(
    fs.map((f) => [f.pref, f.codes]),
    [
      ["奈良県", ["2920100"]],
      ["和歌山県", ["3020500", "3036100"]],
    ],
  );
  // 文は寄る県の危険警報以上だけ (注意報・ほかの県は入れない)
  assert.deepEqual(fs[1].lines, ["レベル４土砂災害危険警報: 和歌山県 有田市・有田川町"]);
  assert.deepEqual(warnFocuses({ reported_at: "", areas: { "2720000": [k("レベル３大雨警報")] } }), []);
  assert.deepEqual(warnFocuses(null), []);
});

test("the map zooms in for the last part of each minute, visiting each prefecture in turn", () => {
  const fs = [
    { pref: "奈良県", codes: ["a"], lines: [] },
    { pref: "和歌山県", codes: ["b"], lines: [] },
  ];
  assert.equal(warnFocusAt(fs, 30_000), null);
  assert.equal(warnFocusAt(fs, 50_000)?.pref, "奈良県");
  assert.equal(warnFocusAt(fs, 60_000 + 50_000)?.pref, "和歌山県");
  assert.equal(warnFocusAt(fs, 120_000 + 50_000)?.pref, "奈良県");
  assert.equal(warnFocusAt([], 50_000), null);
});
