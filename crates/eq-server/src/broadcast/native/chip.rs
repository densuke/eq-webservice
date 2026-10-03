//! 上部バーの右に出す状態の札 (docs/broadcast-status.md)。BGM の曲名の左に置く。
//! 左の表示 ([平時]・[テスト]) にかかるときは、札を優先して曲名を出さない。

use tiny_skia::Pixmap;

use super::paint::{rrect, TEXT};
use super::text::Text;
use crate::broadcast::status::Notice;

/// 混雑中の札の地 (琥珀) と文字
pub const BUSY_BG: [u8; 3] = [0xff, 0xb3, 0x00];
const BUSY_FG: [u8; 3] = [0x2a, 0x1a, 0x00];
/// 途切れた札の地 (灰) と文字
pub const OUTAGE_BG: [u8; 3] = [0x4a, 0x55, 0x63];
const OUTAGE_FG: [u8; 3] = TEXT;

const PX: f32 = 12.0;
const PAD_X: f32 = 8.0;
const TOP: f32 = 9.0;
const HEIGHT: f32 = 22.0;
const BASELINE: f32 = 24.0;
/// 曲名と札のあいだ
const GAP: f32 = 16.0;
/// 左の表示 ([テスト] まで) がここまで来ている。札の左端はここより右に置く
const LEFT_LIMIT: f32 = 340.0;

/// 曲名の幅 (出さなければ None) と札の幅から、曲名を出すかと、札の右端を決める。
/// right は、右端の表示 (配信元の名前) の左の端
pub fn layout(right: f32, bgm_w: Option<f32>, chip_w: f32) -> (bool, f32) {
    match bgm_w {
        Some(w) if right - w - GAP - chip_w >= LEFT_LIMIT => (true, right - w - GAP),
        _ => (false, right),
    }
}

/// 札の幅 (文字の幅 + 左右の余白)
pub fn width(text: &mut Text, notice: &Notice) -> f32 {
    text.width(&notice.text(), PX) + 2.0 * PAD_X
}

/// 札を、右端が right になるように描く
pub fn draw(pm: &mut Pixmap, text: &mut Text, notice: &Notice, right: f32) {
    let (bg, fg) = match notice {
        Notice::Busy => (BUSY_BG, BUSY_FG),
        Notice::Outage(_) => (OUTAGE_BG, OUTAGE_FG),
    };
    let w = width(text, notice);
    rrect(pm, right - w, TOP, w, HEIGHT, 5.0, bg, 1.0);
    text.draw(pm, &notice.text(), right - w + PAD_X, BASELINE, PX, fg);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chip_sits_left_of_the_bgm_and_gives_way_when_there_is_no_room() {
        // 曲名が無い: 札が右端
        assert_eq!(layout(1264.0, None, 250.0), (false, 1264.0));
        // 曲名がある: 曲名 + 余白の左に札
        assert_eq!(layout(1264.0, Some(200.0), 250.0), (true, 1264.0 - 200.0 - GAP));
        // 曲名が長くて札が左の表示 (340) にかかる: 曲名を出さず、札は右端
        assert_eq!(layout(1264.0, Some(700.0), 250.0), (false, 1264.0));
        // ちょうど収まる
        let w = 1264.0 - GAP - LEFT_LIMIT - 250.0;
        assert_eq!(layout(1264.0, Some(w), 250.0), (true, 1264.0 - w - GAP));
    }
}
