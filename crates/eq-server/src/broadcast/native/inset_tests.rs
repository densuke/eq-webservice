//! 離島の別枠: 南西諸島は日本全体のときいつも、小笠原は揺れや震央があるときだけ出す

use super::draw::INSET_LINE;
use super::placed::Placed;
use super::tests::{quake, renderer, rgb, scene};
use super::text::Text;
use crate::quake::Scale;

/// 別枠の左の枠線の画素 (枠の 1px 外)
fn border_px(r: super::layout_resolve::Rect) -> (u32, u32) {
    ((r.x - 1.0) as u32, (r.y + r.h / 2.0) as u32)
}

#[test]
fn the_ogasawara_inset_is_shown_only_when_something_is_there() {
    let placed = Placed::builtin().unwrap();
    let (oki, oga) = (placed.inset.unwrap(), placed.ogasawara.unwrap());
    let mut r = renderer(Text::none());
    // 平時: 南西諸島だけ
    let pm = r.render(&scene(None, &[], None, None));
    assert_eq!(rgb(&pm, border_px(oki)), INSET_LINE);
    assert_ne!(rgb(&pm, border_px(oga)), INSET_LINE);
    // 小笠原の近くが震央の地震 (2026-10-25 小笠原諸島西方沖): 小笠原も出す
    let q = quake(Scale::S3, &[("東京都", Scale::S3)], Some((28.0, 141.0)));
    let pm = r.render(&scene(Some(&q), &[], None, None));
    assert_eq!(rgb(&pm, border_px(oga)), INSET_LINE);
    // 本州の地震では出さない
    let q = quake(Scale::S3, &[("千葉県", Scale::S3)], Some((35.7, 140.8)));
    let pm = r.render(&scene(Some(&q), &[], None, None));
    assert_ne!(rgb(&pm, border_px(oga)), INSET_LINE);
}
