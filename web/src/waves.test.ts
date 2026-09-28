import { test } from "node:test";
import assert from "node:assert/strict";
import { arrivalSec, distanceKm, geoCircle, surfaceRadiusKm, VP_KM_S } from "./waves.ts";

test("wave has not surfaced before reaching the depth", () => {
  assert.equal(surfaceRadiusKm(VP_KM_S, 65, 5), null);
  assert.equal(surfaceRadiusKm(VP_KM_S, 0, 0), null);
  assert.ok(Math.abs(surfaceRadiusKm(VP_KM_S, 0, 10)! - 65) < 1e-9);
});

test("arrival time is consistent with radius", () => {
  const t = arrivalSec(VP_KM_S, 40, 100);
  assert.ok(Math.abs(surfaceRadiusKm(VP_KM_S, 40, t)! - 100) < 1e-6);
});

test("great-circle distance Tokyo-Osaka is about 400km", () => {
  const d = distanceKm(35.681, 139.767, 34.702, 135.495);
  assert.ok(d > 390 && d < 410, `${d}`);
});

test("geo circle points are at the requested distance", () => {
  for (const [lon, lat] of geoCircle(38, 142, 150, 12)) {
    assert.ok(Math.abs(distanceKm(38, 142, lat, lon) - 150) < 0.5);
  }
});
