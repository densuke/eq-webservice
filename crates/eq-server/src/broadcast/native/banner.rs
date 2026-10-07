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
const HEADING: &str = "【気象警報】";

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

/// 帯の 1 種別 (見出しと、その本文)。ページをまたぐときは続きのページの先頭に見出しを再掲する
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    /// 「【大津波警報】」のような見出し
    pub heading: String,
    pub body: String,
}

impl Section {
    pub fn new(heading: &str, body: String) -> Self {
        Self {
            heading: heading.into(),
            body,
        }
    }
}

/// 種別ごとに行へ割り、MAX_LINES 行ずつのページにする。adv は 1 字の幅。種別の先頭行には見出しを付け、
/// 種別がページの途中から次のページへ続くときは、続きの先頭行に「見出し(続き)」を付ける。
/// 種別は前の種別の続きの行に並べる (収まるなら同じページ)
fn paginate(sections: &[Section], max_w: f32, adv: &mut impl FnMut(char) -> f32) -> Vec<Vec<String>> {
    let mut pages: Vec<Vec<String>> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    for s in sections {
        let chars: Vec<char> = s.body.chars().collect();
        let mut pos = 0;
        let mut first = true;
        while first || pos < chars.len() {
            let head = if first {
                format!("{} ", s.heading)
            } else if cur.is_empty() {
                format!("{}(続き) ", s.heading)
            } else {
                String::new()
            };
            while chars.get(pos) == Some(&' ') {
                pos += 1;
            }
            let mut w = head.chars().map(&mut *adv).sum::<f32>();
            let mut end = pos;
            // 1 字は必ず進める (幅より広い字でも止まらない)
            while end < chars.len() && (end == pos || w + adv(chars[end]) <= max_w) {
                w += adv(chars[end]);
                end += 1;
            }
            cur.push(format!("{head}{}", chars[pos..end].iter().collect::<String>()));
            pos = end;
            first = false;
            if cur.len() == MAX_LINES {
                pages.push(std::mem::take(&mut cur));
            }
        }
    }
    if !cur.is_empty() {
        pages.push(cur);
    }
    pages
}

/// 帯のページ。1 ページに収まらないとき (ページが 2 つ以上) は、右端のページ番号の分 (PAGE_W) を空けて割り直す
pub fn pages(sections: &[Section], max_w: f32, adv: &mut impl FnMut(char) -> f32) -> Vec<Vec<String>> {
    let p = paginate(sections, max_w, adv);
    if p.len() > 1 {
        paginate(sections, max_w - PAGE_W, adv)
    } else {
        p
    }
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
    draw_pages(pm, text, at, top, &[Section::new(HEADING, lines.join(" ／ "))], now_ms);
}

/// 帯の地 (top の色。特別警報は白い縁) を塗り、種別ごとの文 sections を 2 行ずつのページで描く
pub fn draw_pages(pm: &mut Pixmap, text: &mut Text, at: Rect, top: WarningLevel, sections: &[Section], now_ms: u64) {
    let all = pages(sections, at.w - PAD_X * 2.0, &mut |c| {
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
    /// 全角 1 字 = 1 の幅で、1 行 max_w 字
    fn pg(sections: &[(&str, &str)], max_w: f32) -> Vec<Vec<String>> {
        let ss: Vec<Section> = sections.iter().map(|(h, b)| Section::new(h, b.to_string())).collect();
        paginate(&ss, max_w, &mut adv)
    }

    #[test]
    fn a_short_text_is_one_page_with_the_heading() {
        let p = pg(&[("【気象警報】", "あいうえお")], 20.0);
        assert_eq!(p, vec![vec!["【気象警報】 あいうえお".to_string()]]);
    }

    #[test]
    fn a_long_text_wraps_and_keeps_every_character() {
        let body = "あいうえおかきくけこさしすせそたちつてと";
        let p = pg(&[("【気象警報】", body)], 12.0);
        let rows: Vec<&String> = p.iter().flatten().collect();
        assert!(rows.len() > 2 && rows.iter().all(|r| r.chars().map(adv).sum::<f32>() <= 12.0));
        // 見出しの再掲と空白を除くと元の本文になる
        let joined: String = rows
            .iter()
            .map(|r| r.replace("【気象警報】(続き) ", "").replace("【気象警報】 ", ""))
            .collect();
        assert_eq!(joined, body);
    }

    #[test]
    fn an_overflowing_section_repeats_its_heading_at_the_top_of_the_next_page() {
        let w = 10.0;
        let p = pg(&[("【特別警報】", "あいうえおかきくけこさしすせそたちつてと")], w);
        assert!(p.len() >= 2, "{p:?}");
        assert!(p[0][0].starts_with("【特別警報】 "), "{p:?}");
        assert!(!p[0][1].starts_with("【特別警報】"), "{p:?}");
        for page in &p[1..] {
            assert!(page[0].starts_with("【特別警報】(続き) "), "{p:?}");
        }
    }

    #[test]
    fn a_section_that_ends_exactly_at_the_page_end_does_not_make_the_next_one_a_continuation() {
        let w = 12.0;
        // 大津波警報は 2 行ちょうど (1 ページ目を満たす)、津波警報は 2 ページ目から始まる
        let p = pg(
            &[
                ("【大津波警報】", "あいうえおかきくけこさしすせそた"),
                ("【津波警報】", "つてと"),
            ],
            w,
        );
        assert_eq!(p.len(), 2, "{p:?}");
        assert!(p[0].iter().all(|r| !r.starts_with("【津波警報】")), "{p:?}");
        assert_eq!(p[1], vec!["【津波警報】 つてと".to_string()]);
    }

    #[test]
    fn short_sections_share_a_page() {
        let p = pg(&[("【大津波警報】", "あい"), ("【津波警報】", "うえ")], 30.0);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].len(), 2);
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
