import { test } from "node:test";
import assert from "node:assert/strict";
import { holdMs, subMapState } from "./hold.ts";
import { SUB_MAP } from "./layout.ts";

test("hold time follows the max scale (boundaries fall on the right side)", () => {
  const want: [number, number][] = [[10, 60], [20, 60], [30, 180], [40, 180], [45, 600], [50, 600], [55, 900], [60, 900], [70, 900], [-1, 180], [0, 180]];
  for (const [scale, sec] of want) assert.equal(holdMs(scale, SUB_MAP), sec * 1000, `scale ${scale}`);
});

test("subMapState is solid inside the hold time, faded after, null when not a quake", () => {
  const g = { key: "a", kind: "quake", updatedAt: 1000 };
  assert.deepEqual(subMapState(1000 + 60000, g, 20, SUB_MAP), { key: "a", faded: false });
  assert.deepEqual(subMapState(1000 + 60001, g, 20, SUB_MAP), { key: "a", faded: true });
  assert.deepEqual(subMapState(1000, { ...g, kind: "eew" }, 70, SUB_MAP), { key: "a", faded: false });
  assert.equal(subMapState(1000, undefined, 20, SUB_MAP), null);
  assert.equal(subMapState(1000, { ...g, kind: "tsunami" }, 20, SUB_MAP), null);
});
