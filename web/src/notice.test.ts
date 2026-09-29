// 配信の音の知らせ (docs/broadcast-v2.md の W1)。形は eq-server の mixer と合わせてあるので、変えるときは文書を先に直す。
import { test } from "node:test";
import assert from "node:assert/strict";
import { bgmNotice, changed } from "./notice.ts";

test("平時で BGM が ON なら、音量つきで流す知らせになる", () => {
  assert.equal(JSON.stringify(bgmNotice(true, 40)), '{"type":"bgm","play":true,"volume":0.4}');
});

test("流さないときは止める知らせになり、音量は含まない", () => {
  assert.equal(JSON.stringify(bgmNotice(false, 40)), '{"type":"bgm","play":false}');
});

test("音量は 0〜1 に収める", () => {
  assert.deepEqual(bgmNotice(true, 250), { type: "bgm", play: true, volume: 1 });
  assert.deepEqual(bgmNotice(true, -5), { type: "bgm", play: true, volume: 0 });
});

test("前と同じ知らせは送らない", () => {
  const a = bgmNotice(true, 40);
  assert.equal(changed(a, bgmNotice(true, 40)), null);
  assert.deepEqual(changed(a, bgmNotice(false, 40)), { type: "bgm", play: false });
  assert.deepEqual(changed(null, a), a);
});
