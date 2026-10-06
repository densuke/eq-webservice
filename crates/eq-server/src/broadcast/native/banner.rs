//! 平時の気象警報の帯 (docs/broadcast-native.md 14 章)。web の #warn-banner と同じ規則・文・色。
//! 画面の上、上部バーのすぐ下に横いっぱいで重ねる。流さず、2 行に収まらない分は「ほか N 件」で省く。

use tiny_skia::Pixmap;

use super::data::{warning_summary, WarnSummary, WarningLevel, Warnings};
use super::draw::W;
use super::paint::rect;
use super::text::Text;

const PER_PREF: usize = 5;
const MAX_LINES: usize = 2;
const PX: f32 = 15.0;
const PAD_X: f32 = 16.0;
const LINE_H: f32 = 20.0;
const PAD_Y: f32 = 5.0;
const WHITE: [u8; 3] = [0xff, 0xff, 0xff];
const PREFIX: &str = "【気象警報】 ";

/// 帯の地の色 (web/public/style.css の .warn-banner)
pub fn banner_color(top: WarningLevel) -> [u8; 3] {
    match top {
        WarningLevel::Danger => [0x7a, 0x1f, 0xa2],
        WarningLevel::Emergency => [0x0c, 0x00, 0x0c],
        _ => [0xb3, 0x26, 0x1e],
    }
}

/// 帯の高さ (行数で決まる)
pub fn banner_height(lines: usize) -> f32 {
    PAD_Y * 2.0 + LINE_H * lines as f32
}

/// 文を幅 max_w の行に割る。max_lines 行に収まらないときは、最後の行を削って「… ほか N 件」を付ける
/// (N は、文の最後まで見えない種類の数)。adv は 1 字の幅
pub fn layout(segments: &[String], max_w: f32, max_lines: usize, adv: &mut impl FnMut(char) -> f32) -> Vec<String> {
    let mut chars: Vec<char> = PREFIX.chars().collect();
    let mut ends = Vec::new();
    for (i, s) in segments.iter().enumerate() {
        if i > 0 {
            chars.extend(" ／ ".chars());
        }
        chars.extend(s.chars());
        ends.push(chars.len());
    }
    let mut width = |cs: &[char]| cs.iter().map(|&c| adv(c)).sum::<f32>();
    let mut lines = Vec::new();
    let mut pos = 0;
    for i in 0..max_lines {
        while chars.get(pos) == Some(&' ') {
            pos += 1;
        }
        let mut end = pos;
        while end < chars.len() && width(&chars[pos..=end]) <= max_w {
            end += 1;
        }
        if end == chars.len() || i + 1 < max_lines {
            lines.push(chars[pos..end].iter().collect());
            pos = end;
            if end == chars.len() {
                break;
            }
            continue;
        }
        // 最後の行で収まらない: 「… ほか N 件」の分を空けて削る
        let suffix = |end: usize| format!("… ほか{}件", ends.iter().filter(|&&e| e > end).count());
        while end > pos + 1 && width(&chars[pos..end]) + width(&suffix(end).chars().collect::<Vec<_>>()) > max_w {
            end -= 1;
        }
        let shown: String = chars[pos..end].iter().collect();
        lines.push(format!("{}{}", shown.trim_end(), suffix(end)));
    }
    lines
}

/// 警報以上があれば帯を描く。bar_bottom は帯を置く上端 (上部バーの下 = 定義の main の上端)、
/// offset_y はそこからの下げ幅 (テスト配信の赤い帯の分)
pub fn draw(pm: &mut Pixmap, text: &mut Text, w: &Warnings, bar_bottom: f32, offset_y: f32) {
    let Some(WarnSummary { top, lines }) = warning_summary(w, PER_PREF) else {
        return;
    };
    let rows = layout(&lines, W as f32 - PAD_X * 2.0, MAX_LINES, &mut |c| {
        text.width(c.encode_utf8(&mut [0; 4]), PX)
    });
    let (y, h) = (bar_bottom + offset_y, banner_height(rows.len()));
    rect(pm, 0.0, y, W as f32, h, banner_color(top), 1.0);
    if top == WarningLevel::Emergency {
        rect(pm, 0.0, y, W as f32, 2.0, WHITE, 1.0);
        rect(pm, 0.0, y + h - 2.0, W as f32, 2.0, WHITE, 1.0);
    }
    for (i, row) in rows.iter().enumerate() {
        let base = y + PAD_Y + 15.0 + LINE_H * i as f32;
        text.draw(pm, row, PAD_X, base, PX, WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 全角 1 字 = 1、半角は 0.5 の幅で数える
    fn adv(c: char) -> f32 {
        if c.is_ascii() {
            0.5
        } else {
            1.0
        }
    }
    fn segs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_short_text_is_one_line_joined_like_the_page() {
        let l = layout(
            &segs(&["レベル３土砂災害警報: 東京都 八丈町", "強風警報: 沖縄県 那覇市"]),
            100.0,
            2,
            &mut adv,
        );
        assert_eq!(
            l,
            ["【気象警報】 レベル３土砂災害警報: 東京都 八丈町 ／ 強風警報: 沖縄県 那覇市"]
        );
    }

    #[test]
    fn a_long_text_wraps_to_two_lines() {
        let l = layout(&segs(&["あいうえおかきくけこ", "さしすせそ"]), 15.0, 2, &mut adv);
        assert_eq!(l.len(), 2);
        assert!(l[0].starts_with("【気象警報】"), "{l:?}");
        assert!(!l[1].starts_with(' ') && l[1].ends_with("さしすせそ"), "{l:?}");
    }

    #[test]
    fn what_does_not_fit_in_two_lines_is_replaced_by_the_count_of_kinds_hidden() {
        let segments = segs(&[
            "あいうえおかきくけこ",
            "さしすせそたちつてと",
            "なにぬねのはひふへほ",
            "まみむめもやゆよ",
        ]);
        let l = layout(&segments, 15.0, 2, &mut adv);
        assert_eq!(l.len(), 2);
        let total = |s: &str| s.chars().map(adv).sum::<f32>();
        assert!(total(&l[1]) <= 15.0, "{l:?}");
        let hidden: usize = l[1]
            .rsplit_once("ほか")
            .unwrap()
            .1
            .trim_end_matches('件')
            .parse()
            .unwrap();
        // 途中まで見えている種類は、最後まで見えないので数に入れる
        let joined = l.concat();
        let shown_kinds = segments.iter().filter(|s| joined.contains(s.as_str())).count();
        assert!(hidden >= 1 && hidden == 4 - shown_kinds, "{l:?}");
    }

    #[test]
    fn colors_follow_the_page() {
        assert_eq!(banner_color(WarningLevel::Warning), [0xb3, 0x26, 0x1e]);
        assert_eq!(banner_color(WarningLevel::Danger), [0x7a, 0x1f, 0xa2]);
        assert_eq!(banner_color(WarningLevel::Emergency), [0x0c, 0x00, 0x0c]);
    }
}
