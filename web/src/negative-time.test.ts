// 1970 年より前の記録の場面は受信時刻が負になる。それでも地震が優先の対象に入り、新しい方が先頭に来ること。
import { test } from "node:test";
import assert from "node:assert/strict";
import { GroupStore } from "./groups.ts";
import { priorityGroups } from "./quakes.ts";
import { app } from "./state.ts";
import type { QuakeEvent } from "./types.ts";

const quake = (id: string, minute: string, ms: number): QuakeEvent => ({
  id,
  source: "test",
  received_at_ms: ms,
  kind: "quake",
  info_type: "detail_scale",
  origin_time: `${minute}:00`,
  origin_time_ms: ms - 20_000,
  issued_at: "",
  hypocenter: { name: "長野県北部", latitude: 36.5, longitude: 138.2, depth_km: 3, magnitude: 5 },
  max_scale: 47,
  domestic_tsunami: "None",
  points: [],
  pref_max: [],
  comment: "",
});

test("with received times before 1970, the newest quake comes first", () => {
  const t = Date.UTC(1966, 0, 23);
  app.world = { store: new GroupStore(), tsunami: null, userquake: null };
  app.world.store.add(quake("1", "1966/01/23 20:15", t));
  app.world.store.add(quake("2", "1966/01/23 20:16", t + 60_000));
  const groups = priorityGroups(t + 70_000);
  assert.equal(groups.length, 2);
  assert.equal(groups[0].events[0].id, "2");
});
