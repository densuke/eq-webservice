import { test } from "node:test";
import assert from "node:assert/strict";
import { arrivalSec, capKm, distanceKm, geoCircle, surfaceRadiusKm, VP_KM_S, VS_KM_S, WAVE_LIMIT_SEC, WAVE_MAX_SEC, waveLifeSec } from "./waves.ts";

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

test("wave circles stop growing at the cap; a wave not yet surfaced stays null", () => {
  assert.equal(capKm(450, 120), 120);
  assert.equal(capKm(80, 120), 80);
  assert.equal(capKm(null, 120), null);
});

test("deep or far waves are drawn until the S wave reaches the shaken area", () => {
  // 浅く近い地震は今までどおり 3 分
  assert.equal(waveLifeSec(10, 300), WAVE_MAX_SEC);
  // 2026-10-25 小笠原諸島西方沖 (深さ 530km): 青森 (約 1400km) まで約 400 秒
  const life = waveLifeSec(530, 1400);
  assert.ok(life > 395 && life < 410, `${life}`);
  assert.ok(surfaceRadiusKm(VS_KM_S, 530, life)! >= 1400);
  assert.equal(waveLifeSec(600, 3000), WAVE_LIMIT_SEC);
});
