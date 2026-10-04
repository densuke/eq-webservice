import { test } from "node:test";
import assert from "node:assert/strict";
import { enqueue, voiceUrl, MAX_VOICES, isPriorityTsunami, announceBody, announceUrl, tsunamiHistory, voiceRoute } from "./voice.ts";
import type { Item } from "./voice.ts";
import type { EqEvent } from "./types.ts";

const it = (id: string, group: string): Item => ({ id, group });

test("empty queue: item is appended", () => {
  assert.deepEqual(enqueue([], it("a", "g1")), [it("a", "g1")]);
});

test("different groups queue in order", () => {
  const q = enqueue([it("a", "g1")], it("b", "g2"));
  assert.deepEqual(q, [it("a", "g1"), it("b", "g2")]);
});

test("same group replaces the waiting item at its position", () => {
  const q = enqueue([it("a", "g1"), it("b", "g2"), it("c", "g3")], it("b2", "g2"));
  assert.deepEqual(q, [it("a", "g1"), it("b2", "g2"), it("c", "g3")]);
});

test("input array is not mutated", () => {
  const input = [it("a", "g1"), it("b", "g2")];
  const copy = structuredClone(input);
  const out = enqueue(input, it("c", "g3"));
  assert.notEqual(out, input);
  assert.deepEqual(input, copy);
  enqueue(input, it("b2", "g2"));
  assert.deepEqual(input, copy);
});

test("overflow drops the oldest, keeping the last MAX_VOICES", () => {
  assert.equal(MAX_VOICES, 4);
  let q: Item[] = [];
  for (const n of [1, 2, 3, 4, 5]) q = enqueue(q, it(`i${n}`, `g${n}`));
  assert.deepEqual(q.map((x) => x.id), ["i2", "i3", "i4", "i5"]);
});

test("voiceUrl encodes the id and resolves relative to base", () => {
  assert.equal(voiceUrl("a/b c", "https://eq.example/app/"), "https://eq.example/app/api/tts/event/a%2Fb%20c");
  assert.equal(voiceUrl("x", "https://eq.example/"), "https://eq.example/api/tts/event/x");
});

const pr = (id: string, group: string): Item => ({ id, group, priority: true });
const ids = (q: Item[]) => q.map((x) => x.id);

test("priority goes before waiting normals, after waiting priorities", () => {
  let q = enqueue([], it("n1", "g1"));
  q = enqueue(q, it("n2", "g2"));
  q = enqueue(q, pr("p1", "g3"));
  assert.deepEqual(ids(q), ["p1", "n1", "n2"]);
  q = enqueue(q, pr("p2", "g4"));
  assert.deepEqual(ids(q), ["p1", "p2", "n1", "n2"]);
});

test("priority replacing the same group moves to the priority position", () => {
  const q = enqueue([it("n1", "g1"), it("n2", "g2"), it("n3", "g3")], pr("p", "g3"));
  assert.deepEqual(ids(q), ["p", "n1", "n2"]);
});

test("cap with priority drops normals from the front first", () => {
  const q = enqueue([it("n1", "g1"), it("n2", "g2"), it("n3", "g3"), it("n4", "g4")], pr("p", "g5"));
  assert.deepEqual(ids(q), ["p", "n2", "n3", "n4"]);
});

test("cap with all priority drops the oldest", () => {
  let q: Item[] = [];
  for (const n of [1, 2, 3, 4, 5]) q = enqueue(q, pr(`p${n}`, `g${n}`));
  assert.deepEqual(ids(q), ["p2", "p3", "p4", "p5"]);
});

test("items without priority behave as before", () => {
  const q = enqueue([it("a", "g1"), it("b", "g2"), it("c", "g3")], it("b2", "g2"));
  assert.deepEqual(q, [it("a", "g1"), it("b2", "g2"), it("c", "g3")]);
});

const area = (grade: string) => ({ grade });

test("isPriorityTsunami: warning grades are priority", () => {
  assert.equal(isPriorityTsunami({ kind: "tsunami", areas: [area("watch"), area("major_warning")] }), true);
  assert.equal(isPriorityTsunami({ kind: "tsunami", areas: [area("warning")] }), true);
});

test("isPriorityTsunami: watch-only, cancelled and non-tsunami are not", () => {
  assert.equal(isPriorityTsunami({ kind: "tsunami", areas: [area("watch")] }), false);
  assert.equal(isPriorityTsunami({ kind: "tsunami", cancelled: true, areas: [area("warning")] }), false);
  assert.equal(isPriorityTsunami({ kind: "quake", areas: [area("warning")] }), false);
});

const ev = (id: string) => ({ id, kind: "quake", source: "t", received_at_ms: 0 }) as unknown as EqEvent;

test("announceBody: priors are the events before e in group order", () => {
  const [a, b, c, d] = ["a", "b", "c", "d"].map(ev);
  const out = JSON.parse(announceBody(c!, [a!, b!, c!, d!]));
  assert.deepEqual(Object.keys(out).sort(), ["event", "priors"]);
  assert.deepEqual(out.event, c);
  assert.deepEqual(out.priors, [a, b]);
});

test("announceBody: e not in group means all events are priors", () => {
  const [a, b, e] = ["a", "b", "e"].map(ev);
  assert.deepEqual(JSON.parse(announceBody(e!, [a!, b!])).priors, [a, b]);
});

test("announceBody: priors capped to the last 300", () => {
  const group = Array.from({ length: 350 }, (_, i) => ev(`x${i}`));
  const e = ev("e");
  const priors = JSON.parse(announceBody(e, [...group, e])).priors;
  assert.equal(priors.length, 300);
  assert.equal(priors[0].id, "x50");
  assert.equal(priors[299].id, "x349");
});

test("announceUrl resolves relative to base", () => {
  assert.equal(announceUrl("https://eq.example/app/"), "https://eq.example/app/api/tts/announce");
});

test("tsunamiHistory: collects tsunami events from all groups in issued order", () => {
  // 津波予報は報ごとに別のまとまりになるので、続報の比較用に発表時刻の順で集め直す
  const t = (id: string, issued_at: string) => ({ id, kind: "tsunami", issued_at, cancelled: false, areas: [] }) as unknown as EqEvent;
  const q = { id: "q", kind: "quake" } as unknown as EqEvent;
  const groups = [{ events: [t("b", "2024/01/01 16:22:32")] }, { events: [q] }, { events: [t("a", "2024/01/01 16:12:54")] }];
  assert.deepEqual(
    tsunamiHistory(groups).map((e) => e.id),
    ["a", "b"],
  );
});

test("voiceRoute: demo and history replays (source demo) read from the cache, real events by id", () => {
  // デモと履歴の再生は報を live として流し、id に #demoN を付ける。サーバはその id を知らないので本文つきで送る
  const ev = (source: string) => ({ id: "x#demo1", kind: "quake", source }) as unknown as EqEvent;
  assert.equal(voiceRoute(ev("demo"), true), "announce");
  assert.equal(voiceRoute(ev("p2pquake"), true), "get");
  // 黙って流す場合 (位置を飛ばした・再生開始時の早送り) は読まない
  assert.equal(voiceRoute(ev("demo"), false), null);
  assert.equal(voiceRoute(ev("p2pquake"), false), null);
});

test("browser playback asks for the smaller 22.05 kHz wav, the mixer url stays 44.1 kHz", () => {
  // 遠い回線でも早く届くよう、ブラウザで鳴らすときだけ ?rate=22050 を付ける
  assert.equal(voiceUrl("a/b", "https://eq.example/", 22050), "https://eq.example/api/tts/event/a%2Fb?rate=22050");
  assert.equal(voiceUrl("a/b", "https://eq.example/"), "https://eq.example/api/tts/event/a%2Fb");
  assert.equal(announceUrl("https://eq.example/app/", 22050), "https://eq.example/app/api/tts/announce?rate=22050");
});
