import { test } from "node:test";
import assert from "node:assert/strict";
import { nextTrack } from "./playlist.ts";

test("tracks play in file name order and wrap around", () => {
  const files = ["b.mp3", "a.mp3", "c.ogg"];
  assert.equal(nextTrack(files, null), "a.mp3");
  assert.equal(nextTrack(files, "a.mp3"), "b.mp3");
  assert.equal(nextTrack(files, "c.ogg"), "a.mp3");
  assert.equal(nextTrack([], null), null);
});

test("when the current file was replaced or removed, play continues from the next name", () => {
  // 流していた b.mp3 が差し替えで消えても、名前順でその次 (c.ogg) から
  assert.equal(nextTrack(["a.mp3", "c.ogg"], "b.mp3"), "c.ogg");
});
