import { test } from "node:test";
import assert from "node:assert/strict";
import { candidates, placeBadges } from "./thin.ts";

const r = (x0: number, y0: number, x1: number, y1: number) => ({ x0, y0, x1, y1 });
const sea = () => false;
const WIDE = r(-1000, -1000, 1000, 1000);
const stay = { dx: 0, dy: 0, line: null };
// 重なる 2 枚: 点はそれぞれの札の中心
const A = r(0, 0, 40, 20);
const B = r(10, 5, 50, 25);
const dots: [number, number][] = [[20, 10], [30, 15]];
const movedB = (p: { dx: number; dy: number }) => r(B.x0 + p.dx, B.y0 + p.dy, B.x1 + p.dx, B.y1 + p.dy);
const touches = (a: ReturnType<typeof r>, b: ReturnType<typeof r>) => a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1;

test("boxes that do not overlap are not moved", () => {
  const boxes = [r(0, 0, 10, 10), r(10, 0, 20, 10), r(0, 20, 10, 30)];
  assert.deepEqual(placeBadges(boxes, [[5, 5], [15, 5], [5, 25]], [], WIDE, sea), [stay, stay, stay]);
});

test("the lower priority of two overlapping boxes moves to a free spot and gets a leader line", () => {
  const [a, b] = placeBadges([A, B], dots, [], WIDE, sea);
  assert.deepEqual(a, stay);
  assert.ok(b && b.line);
  assert.ok(!touches(movedB(b), A));
});

test("a box overlapping a blocker moves even if it comes first", () => {
  const [a] = placeBadges([A], [[20, 10]], [r(10, 5, 30, 15)], WIDE, sea);
  assert.ok(a && a.line);
});

test("sea spots are preferred over land spots", () => {
  const [, onSea] = placeBadges([A, B], dots, [], WIDE, sea);
  const [, avoided] = placeBadges([A, B], dots, [], WIDE, (b) => b.y0 >= 25);
  assert.notDeepEqual(onSea, avoided);
});

test("a spot outside the map is not used", () => {
  const [, b] = placeBadges([A, B], dots, [], r(0, 0, 60, 30), sea);
  // 地図 (0..60 x 0..30) の中に置ける空きは無い
  assert.equal(b, null);
});

test("a land spot is used when there is no sea spot", () => {
  const [, b] = placeBadges([A, B], dots, [], WIDE, () => true);
  assert.ok(b && b.line);
});

test("a box is dropped only when no spot is free", () => {
  assert.deepEqual(placeBadges([A], [[20, 10]], [r(-1000, -1000, 1000, 1000)], WIDE, sea), [null]);
});

test("a moved box does not cover another city's dot", () => {
  const [, b] = placeBadges([A, B], dots, [], WIDE, sea);
  assert.ok(b);
  const m = movedB(b);
  assert.ok(!(m.x0 < 25 && 15 < m.x1 && m.y0 < 15 && 5 < m.y1));
});

test("candidates keep the box size and stay clear of the dot", () => {
  const cs = candidates(A, [20, 10]);
  assert.equal(cs.length, 24);
  for (const c of cs) {
    assert.equal(c.x1 - c.x0, 40);
    assert.equal(c.y1 - c.y0, 20);
    assert.ok(!(c.x0 < 20 && 20 < c.x1 && c.y0 < 10 && 10 < c.y1));
  }
});

// 警報以上の区域 (札で隠さない)
const inWarn = (z: ReturnType<typeof r>) => (b: ReturnType<typeof r>) => touches(b, z);

test("without warnings nothing changes (warned is never true)", () => {
  const boxes = [r(0, 0, 10, 10), r(10, 0, 20, 10)];
  assert.deepEqual(placeBadges(boxes, [[5, 5], [15, 5]], [], WIDE, sea, () => false), [stay, stay]);
});

test("a box over a warning area moves off it and gets a leader line", () => {
  const zone = r(0, 0, 60, 40);
  const [p] = placeBadges([A], [[20, 10]], [], WIDE, sea, inWarn(zone));
  assert.ok(p && p.line);
  const m = r(A.x0 + p.dx, A.y0 + p.dy, A.x1 + p.dx, A.y1 + p.dy);
  assert.ok(!touches(m, zone));
});

test("the box stays where it is when every spot is blocked by the warning", () => {
  const [p] = placeBadges([A], [[20, 10]], [], WIDE, sea, () => true);
  assert.deepEqual(p, stay);
});
