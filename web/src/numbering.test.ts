import { test } from "node:test";
import assert from "node:assert/strict";
import { assignNumbers } from "./numbering.ts";

test("numbers follow the order of occurrence and stay stable", () => {
  const a = assignNumbers(new Map(), [{ key: "e1" }, { key: "e2" }]);
  assert.deepEqual([...a], [["e1", 1], ["e2", 2]]);
  // 新しい地震は続きの番号。既存の番号は変えない
  const b = assignNumbers(a, [{ key: "e1" }, { key: "e2" }, { key: "e3" }]);
  assert.deepEqual([...b], [["e1", 1], ["e2", 2], ["e3", 3]]);
  // 古いものが外れても番号を詰めない
  const c = assignNumbers(b, [{ key: "e2" }, { key: "e3" }, { key: "e4" }]);
  assert.deepEqual([...c], [["e2", 2], ["e3", 3], ["e4", 4]]);
});

test("a quake report linked to an eew shares its number", () => {
  const a = assignNumbers(new Map(), [{ key: "e1" }, { key: "q1", linkedTo: "e1" }, { key: "q2" }]);
  assert.deepEqual([...a], [["e1", 1], ["q1", 1], ["q2", 2]]);
});

test("numbering restarts from 1 once nothing is active", () => {
  const a = assignNumbers(new Map([["e1", 5]]), []);
  assert.equal(a.size, 0);
  assert.deepEqual([...assignNumbers(a, [{ key: "e9" }])], [["e9", 1]]);
});
