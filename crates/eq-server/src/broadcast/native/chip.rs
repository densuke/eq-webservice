//! 上部バーの右に出す状態の札 (docs/broadcast-status.md)。BGM の曲名の左に置く。
//! 左の表示 ([平時]・[テスト]) にかかるときは、札を優先して曲名・同接を出さない (並べ方は viewers.rs)。

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
/// 札の上端・文字のベースライン (上部バーの上端からの距離)
const TOP: f32 = 9.0;
const HEIGHT: f32 = 22.0;
const BASELINE: f32 = 24.0;

/// 札の幅 (文字の幅 + 左右の余白)
pub fn width(text: &mut Text, notice: &Notice) -> f32 {
    text.width(&notice.text(), PX) + 2.0 * PAD_X
}

/// 札を、右端が right になるように描く。bar_y は上部バーの上端
pub fn draw(pm: &mut Pixmap, text: &mut Text, notice: &Notice, right: f32, bar_y: f32) {
    let (bg, fg) = match notice {
        Notice::Busy => (BUSY_BG, BUSY_FG),
        Notice::Outage(_) => (OUTAGE_BG, OUTAGE_FG),
    };
    let w = width(text, notice);
    rrect(pm, right - w, bar_y + TOP, w, HEIGHT, 5.0, bg, 1.0);
    text.draw(pm, &notice.text(), right - w + PAD_X, bar_y + BASELINE, PX, fg);
}
