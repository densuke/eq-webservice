import { test } from "node:test";
import assert from "node:assert/strict";
import { clockParts } from "./clock.ts";

test("splits a time into JST parts regardless of the browser time zone", () => {
  // 2026-09-28T20:25:52Z = 2026/09/29 05:25:52 JST (火)
  assert.deepEqual(clockParts(Date.UTC(2026, 8, 28, 20, 25, 52)), {
    year: "2026",
    month: "09",
    day: "29",
    weekday: "火",
    hm: "05:25",
    sec: "52",
  });
  // 年をまたぐ
  assert.deepEqual(clockParts(Date.UTC(2026, 11, 31, 15, 0, 0)), {
    year: "2027",
    month: "01",
    day: "01",
    weekday: "金",
    hm: "00:00",
    sec: "00",
  });
});
