//! 平時の気象警報の帯 (docs/broadcast-native.md 14 章)。web の #warn-banner と同じ規則・文・色。
//! 定義の banners (上部バーのすぐ下、横いっぱい。地図と右の列はその下) の矩形の中に描く。流さない。
//! 2 行に収まらないときは、2 行ずつのページに分け、PAGE_MS ごとに置き換える (右端に「1/3」)。
//! 警報が無いときは落ち着いた色で「ありません」、地震の画面では描かない (矩形は空のまま)。

use tiny_skia::Pixmap;

use super::data::{warning_summary, WarnSummary, WarningLevel, Warnings};
use super::layout_resolve::Rect;
use super::paint::{rect, MUTED};
use super::text::Text;

const PER_PREF: usize = 5;
const MAX_LINES: usize = 2;
const PX: f32 = 15.0;
/// 1 ページを出している時間 (ミリ秒)
pub const PAGE_MS: u64 = 8000;
/// ページ番号 (「10/12」まで) のために右端に空ける幅と、その字の大きさ
const PAGE_W: f32 = 56.0;
const PAGE_PX: f32 = 13.0;
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

/// 帯の高さ (行数で決まる。2 行のときが定義の banners の高さ)
pub fn banner_height(lines: usize) -> f32 {
    PAD_Y * 2.0 + LINE_H * lines as f32
}

/// 文を幅 max_w の行に割る (行数の上限は無い)。adv は 1 字の幅
pub fn layout(prefix: &str, segments: &[String], max_w: f32, adv: &mut impl FnMut(char) -> f32) -> Vec<String> {
    let mut chars: Vec<char> = prefix.chars().collect();
    for (i, s) in segments.iter().enumerate() {
        if i > 0 {
            chars.extend(" ／ ".chars());
        }
        chars.extend(s.chars());
    }
    let mut lines = Vec::new();
    let mut pos = 0;
    while pos < chars.len() {
        while chars.get(pos) == Some(&' ') {
            pos += 1;
        }
        let mut end = pos;
        let mut w = 0.0;
        // 1 字は必ず進める (幅より広い字でも止まらない)
        while end < chars.len() && (end == pos || w + adv(chars[end]) <= max_w) {
            w += adv(chars[end]);
            end += 1;
        }
        lines.push(chars[pos..end].iter().collect());
        pos = end;
    }
    lines
}

/// 行を MAX_LINES 行ずつのページにする。1 ページに収まらないとき (ページが 2 つ以上) は、右端のページ番号の分
/// (PAGE_W) を空けて割り直す
pub fn pages(prefix: &str, segments: &[String], max_w: f32, adv: &mut impl FnMut(char) -> f32) -> Vec<Vec<String>> {
    let mut lines = layout(prefix, segments, max_w, adv);
    if lines.len() > MAX_LINES {
        lines = layout(prefix, segments, max_w - PAGE_W, adv);
    }
    lines.chunks(MAX_LINES).map(<[String]>::to_vec).collect()
}

/// 今出すページ (now_ms は epoch ミリ秒)。PAGE_MS ごとに次へ進み、最後の次は先頭に戻る
pub fn page_at(now_ms: u64, pages: usize) -> usize {
    (now_ms / PAGE_MS) as usize % pages.max(1)
}

/// 帯の矩形 at の中に描く。警報以上があれば帯 (長ければ now_ms で選んだページ)、
/// 警報が無ければ落ち着いた色の「ありません」。w が None (まだ取れていない) なら何も描かない
pub fn draw(pm: &mut Pixmap, text: &mut Text, w: Option<&Warnings>, at: Rect, now_ms: u64) {
    let Some(w) = w else { return };
    let Some(WarnSummary { top, lines }) = warning_summary(w, PER_PREF) else {
        let msg = if w.areas.is_empty() {
            "気象警報・注意報はありません"
        } else {
            "気象警報はありません"
        };
        text.draw(pm, msg, at.x + PAD_X, at.y + at.h / 2.0 + PX * 0.35, PX, MUTED);
        return;
    };
    draw_pages(pm, text, at, top, PREFIX, &lines, now_ms);
}

/// 帯の地 (top の色。特別警報は白い縁) を塗り、先頭に prefix を付けた文 segments を 2 行ずつのページで描く
pub fn draw_pages(
    pm: &mut Pixmap,
    text: &mut Text,
    at: Rect,
    top: WarningLevel,
    prefix: &str,
    segments: &[String],
    now_ms: u64,
) {
    let all = pages(prefix, segments, at.w - PAD_X * 2.0, &mut |c| {
        text.width(c.encode_utf8(&mut [0; 4]), PX)
    });
    let page = page_at(now_ms, all.len());
    rect(pm, at.x, at.y, at.w, at.h, banner_color(top), 1.0);
    if top == WarningLevel::Emergency {
        rect(pm, at.x, at.y, at.w, 2.0, WHITE, 1.0);
        rect(pm, at.x, at.bottom() - 2.0, at.w, 2.0, WHITE, 1.0);
    }
    // 帯は常に 2 行ぶんの高さ。1 行のページは縦の中央に置く
    let rows = &all[page];
    let top = at.y + PAD_Y + LINE_H * (MAX_LINES - rows.len()) as f32 / 2.0;
    for (i, row) in rows.iter().enumerate() {
        let base = top + 15.0 + LINE_H * i as f32;
        text.draw(pm, row, at.x + PAD_X, base, PX, WHITE);
    }
    if all.len() > 1 {
        let label = format!("{}/{}", page + 1, all.len());
        text.draw_right(
            pm,
            &label,
            at.right() - PAD_X,
            at.y + at.h / 2.0 + PAGE_PX * 0.35,
            PAGE_PX,
            WHITE,
        );
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
            PREFIX,
            &segs(&["レベル３土砂災害警報: 東京都 八丈町", "強風警報: 沖縄県 那覇市"]),
            100.0,
            &mut adv,
        );
        assert_eq!(
            l,
            ["【気象警報】 レベル３土砂災害警報: 東京都 八丈町 ／ 強風警報: 沖縄県 那覇市"]
        );
    }

    #[test]
    fn a_long_text_wraps_to_as_many_lines_as_it_needs() {
        let l = layout(PREFIX, &segs(&["あいうえおかきくけこ", "さしすせそ"]), 15.0, &mut adv);
        assert_eq!(l.len(), 2);
        assert!(l[0].starts_with("【気象警報】"), "{l:?}");
        assert!(!l[1].starts_with(' ') && l[1].ends_with("さしすせそ"), "{l:?}");
        // 切り捨てない: 並べ直した全文が元の文と同じ
        let long = segs(&["あいうえおかきくけこ", "さしすせそたちつてと", "なにぬねのはひふへほ"]);
        let rows = layout(PREFIX, &long, 15.0, &mut adv);
        assert!(
            rows.len() > 2 && rows.iter().all(|r| r.chars().map(adv).sum::<f32>() <= 15.0),
            "{rows:?}"
        );
        assert_eq!(
            rows.concat().replace(' ', ""),
            format!("【気象警報】{}", long.join("／"))
        );
    }

    #[test]
    fn two_lines_are_one_page_and_more_are_split_into_pages_of_two_lines() {
        // 全角 1 字 = 10 の幅
        let mut adv10 = |c: char| adv(c) * 10.0;
        let short = pages(
            PREFIX,
            &segs(&["あいうえおかきくけこ", "さしすせそ"]),
            150.0,
            &mut adv10,
        );
        assert_eq!(short.len(), 1);
        let long: Vec<String> = (0..8)
            .map(|i| format!("レベル３大雨警報: 県{i} あいうえおかきくけこ"))
            .collect();
        let ps = pages(PREFIX, &long, 200.0 + PAGE_W, &mut adv10);
        assert!(ps.len() >= 2 && ps.iter().all(|p| (1..=2).contains(&p.len())), "{ps:?}");
        // 2 ページ以上のときは、ページ番号の幅を空けて割る
        assert!(
            ps.iter()
                .flatten()
                .all(|r| r.chars().map(|c| adv(c) * 10.0).sum::<f32>() <= 200.0),
            "{ps:?}"
        );
        // 最後以外のページは 2 行ちょうど
        assert!(ps[..ps.len() - 1].iter().all(|p| p.len() == 2));
    }

    #[test]
    fn the_page_changes_every_eight_seconds_and_wraps_around() {
        let t0 = 1_790_000_000_000 / PAGE_MS * PAGE_MS;
        assert_eq!(page_at(t0, 3), page_at(t0 + 7_999, 3));
        assert_eq!(page_at(t0 + PAGE_MS, 3), (page_at(t0, 3) + 1) % 3);
        assert_eq!(page_at(t0 + 3 * PAGE_MS, 3), page_at(t0, 3));
        // 1 ページ (と 0) は常に先頭
        assert_eq!((page_at(t0, 1), page_at(t0 + PAGE_MS, 1), page_at(t0, 0)), (0, 0, 0));
        // 全ページが順に出る
        let seen: std::collections::BTreeSet<_> = (0..3).map(|i| page_at(t0 + i * PAGE_MS, 3)).collect();
        assert_eq!(seen.len(), 3);
    }

    #[test]
    fn colors_follow_the_page() {
        assert_eq!(banner_color(WarningLevel::Warning), [0xb3, 0x26, 0x1e]);
        assert_eq!(banner_color(WarningLevel::Danger), [0x7a, 0x1f, 0xa2]);
        assert_eq!(banner_color(WarningLevel::Emergency), [0x0c, 0x00, 0x0c]);
    }
}
