//! 天気の札の置き場所: 警報以上 (警報・危険警報・特別警報) の塗りを札で隠さない。
//! 札が警報以上の区域に重なるときだけ、点の周りの空き (海を先に) へ動かして引き出し線でつなぐ。
//! 警報が無い (重ならない) ときは、札は今までの位置のまま。web/src/thin.ts と同じ考え方。

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use tiny_skia::Path;

use super::frame::{BoxRect, Frame};
use super::geo::Shape;

/// 札を点から離す距離 (札の縁から点まで) の候補と、点の周りの 8 方向 (上・下・左・右を先に)
const GAPS: [f32; 3] = [14.0, 30.0, 48.0];
const DIRS: [(f32, f32); 8] = [
    (0.0, -1.0),
    (0.0, 1.0),
    (-1.0, 0.0),
    (1.0, 0.0),
    (-1.0, -1.0),
    (1.0, -1.0),
    (-1.0, 1.0),
    (1.0, 1.0),
];
/// 他の都市の点を札で覆わないための余白
const DOT_R: f32 = 5.0;

/// 札 1 枚: 元の位置の四角と、都市の点
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Card {
    pub rect: BoxRect,
    pub dot: (f32, f32),
}

/// 札の置き場所。動かしたときだけ leader (点から札の縁までの線の先、画面の座標) がある
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub rect: BoxRect,
    pub leader: Option<(f32, f32)>,
}

fn overlap(a: BoxRect, b: BoxRect) -> bool {
    a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
}

fn inside(b: BoxRect, o: BoxRect) -> bool {
    b.0 >= o.0 && b.1 >= o.1 && b.0 + b.2 <= o.0 + o.2 && b.1 + b.3 <= o.1 + o.3
}

/// 点 dot の周りの置き場所 (近い順)。札の大きさは rect と同じ
fn candidates(rect: BoxRect, dot: (f32, f32)) -> impl Iterator<Item = BoxRect> {
    let (w, h) = (rect.2, rect.3);
    GAPS.into_iter().flat_map(move |gap| {
        DIRS.into_iter().map(move |(ux, uy)| {
            let (cx, cy) = (dot.0 + ux * (w / 2.0 + gap), dot.1 + uy * (h / 2.0 + gap));
            (cx - w / 2.0, cy - h / 2.0, w, h)
        })
    })
}

/// 札の置き場所 (cards の順)。
/// 警報以上に重ならない札はそのまま。重なる札だけ、点の周りで warned に重ならず、fixed (凡例・別枠・案内)・
/// ほかの札・ほかの点・地図の外 (bounds) とも重ならない所へ動かす (海を先に)。
/// どこにも置けなければ、隠さず元の位置のまま (警報は見えにくいまま)
pub fn place(
    cards: &[Card],
    fixed: &[BoxRect],
    bounds: BoxRect,
    warned: &dyn Fn(BoxRect) -> bool,
    land: &dyn Fn(BoxRect) -> bool,
) -> Vec<Placed> {
    let mut out: Vec<Placed> = Vec::with_capacity(cards.len());
    for (i, card) in cards.iter().enumerate() {
        let stay = Placed {
            rect: card.rect,
            leader: None,
        };
        if !warned(card.rect) {
            out.push(stay);
            continue;
        }
        let others: Vec<BoxRect> = out
            .iter()
            .map(|p| p.rect)
            .chain(cards[i + 1..].iter().map(|c| c.rect))
            .chain(fixed.iter().copied())
            .chain(
                cards
                    .iter()
                    .enumerate()
                    .filter(|&(j, _)| j != i)
                    .map(|(_, c)| (c.dot.0 - DOT_R, c.dot.1 - DOT_R, DOT_R * 2.0, DOT_R * 2.0)),
            )
            .collect();
        let spots: Vec<BoxRect> = candidates(card.rect, card.dot)
            .filter(|&c| inside(c, bounds) && !warned(c) && !others.iter().any(|&o| overlap(c, o)))
            .collect();
        out.push(match spots.iter().find(|&&c| !land(c)).or(spots.first()) {
            Some(&c) => Placed {
                rect: c,
                leader: Some((card.dot.0.clamp(c.0, c.0 + c.2), card.dot.1.clamp(c.1, c.1 + c.3))),
            },
            None => stay,
        });
    }
    out
}

/// 面の上の区域 (外接矩形は画面の座標) の集まり。札の四角が区域の塗りに掛かるかを調べる
pub struct Zones<'a> {
    frame: &'a Frame,
    items: Vec<(BoxRect, &'a Path)>,
}

impl<'a> Zones<'a> {
    pub fn new(frame: &'a Frame, shapes: impl Iterator<Item = &'a Shape>) -> Zones<'a> {
        let items = shapes
            .filter_map(|s| Some((frame.screen_bounds(&s.path)?, &s.path)))
            .collect();
        Zones { frame, items }
    }

    /// 四角を横 5 x 縦 3 の点で調べ、区域の中に入る点が 1 つでもあれば真。
    /// 外接矩形と重ならない区域は、多角形を調べずに飛ばす
    pub fn hit(&self, r: BoxRect) -> bool {
        self.items.iter().filter(|(b, _)| overlap(*b, r)).any(|(_, path)| {
            (0..5).any(|i| {
                (0..3).any(|j| {
                    let p = (r.0 + r.2 * i as f32 / 4.0, r.1 + r.3 * j as f32 / 2.0);
                    self.frame.path_contains(path, p)
                })
            })
        })
    }
}

/// 面ごと・今/明日ごとの置き場所を覚える。警報・札の大きさ・場所が変わらない間は、置き直さない
#[derive(Default)]
pub struct CardCache {
    slots: HashMap<(usize, bool), (u64, Vec<Placed>)>,
}

impl CardCache {
    /// key が前と同じなら覚えた置き場所を返し、違えば compute で置き直す
    pub fn get(&mut self, slot: (usize, bool), key: u64, compute: impl FnOnce() -> Vec<Placed>) -> &[Placed] {
        let entry = self.slots.entry(slot).or_insert_with(|| (!key, Vec::new()));
        if entry.0 != key {
            *entry = (key, compute());
        }
        &entry.1
    }
}

/// 置き直す必要があるかを決める印。警報以上の区域の並び (codes) と、札と固定物の四角から作る
pub fn signature<'a>(codes: impl Iterator<Item = &'a str>, cards: &[Card], fixed: &[BoxRect], bounds: BoxRect) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for c in codes {
        c.hash(&mut h);
    }
    let rects = cards
        .iter()
        .flat_map(|c| [c.rect, (c.dot.0, c.dot.1, 0.0, 0.0)])
        .chain(fixed.iter().copied())
        .chain([bounds]);
    for r in rects {
        [r.0, r.1, r.2, r.3].map(f32::to_bits).hash(&mut h);
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDE: BoxRect = (-1000.0, -1000.0, 2000.0, 2000.0);
    const NONE: &dyn Fn(BoxRect) -> bool = &|_| false;

    /// 札幌の札 (点 (100, 100) の上に 40 x 22) と、離れたもう 1 つの都市
    fn cards() -> Vec<Card> {
        vec![
            Card {
                rect: (80.0, 70.0, 40.0, 22.0),
                dot: (100.0, 100.0),
            },
            Card {
                rect: (200.0, 70.0, 40.0, 22.0),
                dot: (220.0, 100.0),
            },
        ]
    }

    const ZONE: BoxRect = (60.0, 60.0, 80.0, 50.0); // 札幌の札の下の警報

    fn in_zone(b: BoxRect) -> bool {
        overlap(b, ZONE)
    }

    #[test]
    fn without_a_warning_every_card_stays_where_it_was() {
        let cs = cards();
        let out = place(&cs, &[], WIDE, NONE, NONE);
        assert!(out.iter().zip(&cs).all(|(p, c)| p.rect == c.rect && p.leader.is_none()));
    }

    #[test]
    fn a_card_over_a_warning_moves_off_it_and_gets_a_leader_line() {
        let cs = cards();
        let out = place(&cs, &[], WIDE, &in_zone, NONE);
        let r = out[0].rect;
        assert!(!overlap(r, ZONE));
        let (lx, ly) = out[0].leader.expect("leader");
        // 線の先は札の縁 (四角の上)
        assert!((r.0..=r.0 + r.2).contains(&lx) && (r.1..=r.1 + r.3).contains(&ly));
        // 札の大きさは変わらず、警報に重ならない別の札は動かない
        assert_eq!((r.2, r.3), (40.0, 22.0));
        assert_eq!(out[1].rect, cs[1].rect);
    }

    #[test]
    fn a_moved_card_avoids_fixed_boxes_and_the_other_cards() {
        let cs = cards();
        let fixed = [(0.0, 20.0, 400.0, 40.0)]; // 真上の帯
        let out = place(&cs, &fixed, WIDE, &in_zone, NONE);
        assert!(!overlap(out[0].rect, fixed[0]) && !overlap(out[0].rect, cs[1].rect));
    }

    #[test]
    fn sea_is_preferred_to_land() {
        let cs = cards();
        let sea = place(&cs, &[], WIDE, &in_zone, NONE)[0].rect;
        let avoided = place(&cs, &[], WIDE, &in_zone, &|b| b.1 >= 110.0)[0].rect; // 下は陸
        assert_ne!(sea, avoided);
    }

    #[test]
    fn when_every_spot_is_blocked_the_card_keeps_its_default_position() {
        let cs = cards();
        let out = place(&cs, &[], WIDE, &|_| true, NONE);
        assert_eq!(
            out[0],
            Placed {
                rect: cs[0].rect,
                leader: None
            }
        );
    }

    #[test]
    fn the_cache_recomputes_only_when_the_key_changes() {
        let mut cache = CardCache::default();
        let mut calls = 0;
        for key in [1, 1, 1, 2, 2] {
            cache.get((0, false), key, || {
                calls += 1;
                Vec::new()
            });
        }
        assert_eq!(calls, 2);
        // 別の面・別の番は別に覚える
        cache.get((1, false), 1, || {
            calls += 1;
            Vec::new()
        });
        assert_eq!(calls, 3);
    }
}
