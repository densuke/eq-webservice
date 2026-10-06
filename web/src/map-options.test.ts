import { test } from "node:test";
import assert from "node:assert/strict";
import { baseId } from "./map-ids.ts";

test("下地の id は 1 つ目だけ map-base、2 つ目からは連番を付ける", () => {
  assert.equal(baseId(1), "map-base");
  assert.equal(baseId(2), "map-base-2");
  assert.equal(baseId(3), "map-base-3");
});
