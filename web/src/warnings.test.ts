import { test } from "node:test";
import assert from "node:assert/strict";
import { prefOf, topLevel, warningLevel, warningSummary } from "./warnings.ts";

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
