import { test } from "node:test";
import assert from "node:assert/strict";
import { clusterMarkers, edgePoint, labelSize } from "./cluster.ts";

const m = (label: number, x: number, y = 0, scale = 30, primary = false) => ({ label, x, y, scale, primary });

test("markers closer than the distance merge, labels in time order", () => {
  const c = clusterMarkers([m(4, 0), m(1, 5), m(2, 100), m(3, 3)], 10);
  assert.equal(c.length, 2);
  assert.deepEqual(c[0].labels.map((l) => l.label), [1, 3, 4]);
  assert.deepEqual(c[1].labels.map((l) => l.label), [2]);
});

test("a cluster sits on its primary marker and is primary itself", () => {
  const c = clusterMarkers([m(1, 0), m(2, 4, 0, 50, true)], 10);
  assert.equal(c.length, 1);
  assert.equal(c[0].primary, true);
  assert.deepEqual([c[0].x, c[0].y], [4, 0]);
});

test("stronger shaking gets a larger label", () => {
  assert.ok(labelSize(70) > labelSize(40));
  assert.ok(labelSize(40) > labelSize(10));
  assert.equal(labelSize(-1), labelSize(10));
});

test("offscreen points are placed on the edge toward them", () => {
  // 画面 200x100、余白 10
  assert.equal(edgePoint(100, 50, 200, 100, 10), null);
  const right = edgePoint(500, 50, 200, 100, 10)!;
  assert.deepEqual([right.x, right.y], [190, 50]);
  assert.equal(right.angle, 0);
  const up = edgePoint(100, -300, 200, 100, 10)!;
  assert.deepEqual([up.x, up.y], [100, 10]);
  assert.equal(up.angle, -90);
  // 斜め: 中心からの向きのまま端で止まる
  const corner = edgePoint(-400, 300, 200, 100, 10)!;
  assert.ok(Math.abs(corner.x - 10) < 1e-9 || Math.abs(corner.y - 90) < 1e-9);
  assert.ok(corner.x >= 10 && corner.x <= 190 && corner.y >= 10 && corner.y <= 90);
});
