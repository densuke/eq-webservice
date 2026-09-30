//! テスト配信の表示 (docs/broadcast-native.md 12.2)。消す設定は作らない。
//! 赤い帯と TEST の透かしは、文字 (フォント) が無くても形を描く。

use tiny_skia::Pixmap;

use super::draw::{BAR_H, H, MAP_RECT, W};
use super::paint::rect;
use super::text::Text;

pub const BAND: [u8; 3] = [0xb3, 0x00, 0x1b];
const BAND_TEXT: [u8; 3] = [0xff, 0xff, 0xff];
pub const BAND_H: f32 = 20.0;
const MESSAGE: &str = "テスト配信: 過去の地震の再生です。実際の地震ではありません";
const WATERMARK_ALPHA: f32 = 0.15;

pub fn draw(pm: &mut Pixmap, text: &mut Text) {
    watermark(pm);
    band(pm, text, BAR_H);
    band(pm, text, H as f32 - BAND_H);
    text.draw(pm, "[テスト]", 262.0, 24.0, 12.0, BAND_TEXT);
}

/// 帯と、白い太字の文 (太字は少しずらして重ね描き)
fn band(pm: &mut Pixmap, text: &mut Text, y: f32) {
    rect(pm, 0.0, y, W as f32, BAND_H, BAND, 1.0);
    let x = (W as f32 - text.width(MESSAGE, 13.0)) / 2.0;
    for dx in [0.0, 0.8] {
        text.draw(pm, MESSAGE, x + dx, y + 15.0, 13.0, BAND_TEXT);
    }
}

/// 地図の中央に、四角だけで組んだ大きな「TEST」(重ならない四角なので、うすくても濃さがそろう)
fn watermark(pm: &mut Pixmap) {
    let (mx, my, mw, mh) = MAP_RECT;
    let (w, h, t, gap) = (110.0_f32, 190.0_f32, 30.0_f32, 24.0_f32);
    let total = 4.0 * w + 3.0 * gap;
    let (x0, y0) = (mx as f32 + (mw as f32 - total) / 2.0, my as f32 + (mh as f32 - h) / 2.0);
    let mid = (h - t) / 2.0;
    let t_rects = [(0.0, 0.0, w, t), ((w - t) / 2.0, t, t, h - t)];
    let e_rects = [
        (0.0, 0.0, t, h),
        (t, 0.0, w - t, t),
        (t, mid, w - t - 10.0, t),
        (t, h - t, w - t, t),
    ];
    let s_rects = [
        (0.0, 0.0, w, t),
        (0.0, t, t, mid - t),
        (0.0, mid, w, t),
        (w - t, mid + t, t, mid - t),
        (0.0, h - t, w, t),
    ];
    let letters = [&t_rects[..], &e_rects, &s_rects, &t_rects];
    for (i, rects) in letters.iter().enumerate() {
        let lx = x0 + i as f32 * (w + gap);
        for &(x, y, rw, rh) in *rects {
            rect(pm, lx + x, y0 + y, rw, rh, [0xff, 0xff, 0xff], WATERMARK_ALPHA);
        }
    }
}
