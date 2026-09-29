import { test } from "node:test";
import assert from "node:assert/strict";
import { topLevel, warningLevel } from "./warnings.ts";

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
