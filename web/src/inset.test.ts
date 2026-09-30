import { test } from "node:test";
import assert from "node:assert/strict";
import { insetMarkerPos } from "./inset.ts";

const b = { x0: 0, y0: 0, x1: 100, y1: 80 };

test("a point inside stays; just outside is pulled to the edge (pad inside); far away is skipped", () => {
  assert.deepEqual(insetMarkerPos(50, 40, b, 10, 10, 2), [50, 40]);
  assert.deepEqual(insetMarkerPos(-5, 40, b, 10, 10, 2), [2, 40]);
  assert.deepEqual(insetMarkerPos(105, 90, b, 10, 10, 2), [98, 78]);
  assert.equal(insetMarkerPos(-11, 40, b, 10, 10, 2), null);
  assert.equal(insetMarkerPos(50, 91, b, 10, 10, 2), null);
});
