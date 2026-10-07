//! 重大な警報の枠 (docs/superpowers/specs/2026-10-07-major-alerts-panel.md)。右の列の「履歴」の矩形を、
//! 大津波警報・津波警報・気象の特別警報があるあいだだけ置き換える。何を出すか・色・順序は quake_band と同じ。
//! 種別ごとに見出しの行 + 区域の行。入りきらなければ帯と同じ置き換え式 (PAGE_MS) のページ送り。
//! 割り付けとページ割りは純粋な関数 (layout)。

use tiny_skia::Pixmap;

use super::banner::{banner_color, line_end, page_at};
use super::layout_resolve::Rect;
use super::paint::{rect, TEXT};
use super::quake_band::{QuakeBand, Tone};
use super::text::Text;

const PAD: f32 = 16.0;
const TITLE_H: f32 = 22.0;
const TITLE_PX: f32 = 13.0;
const ROW_H: f32 = 20.0;
const ROW_PX: f32 = 13.0;
const BOTTOM_PAD: f32 = 4.0;
const TITLE: &str = "重大な警報";
const WHITE: [u8; 3] = [0xff, 0xff, 0xff];

/// 枠の 1 行
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    /// 種別の見出し (色は種類で決まる)
    Head(Tone, String),
    /// 区域の文
    Body(String),
}

/// 高さ h の矩形に入る行数。最低 2 (見出し 1 行 + 区域 1 行) は読めるようにする
fn capacity(h: f32) -> usize {
    (((h - TITLE_H - BOTTOM_PAD) / ROW_H).max(0.0) as usize).max(2)
}

/// 種別ごとの見出しと区域の文を、cap 行ずつのページに割り付ける。
/// 区域の文は max_w の幅で区切り (、・空白) の直後で折る。見出しだけがページの末尾に残らないように、
/// 空きが 2 行未満なら次のページから始める。種別がページをまたぐときは、続きの先頭に「見出し(続き)」を付ける
pub fn layout(band: &QuakeBand, max_w: f32, cap: usize, adv: &mut impl FnMut(char) -> f32) -> Vec<Vec<Row>> {
    let cap = cap.max(2);
    let mut pages: Vec<Vec<Row>> = Vec::new();
    let mut cur: Vec<Row> = Vec::new();
    for (s, &tone) in band.sections.iter().zip(&band.tones) {
        let chars: Vec<char> = s.body.chars().collect();
        let mut pos = 0;
        let mut first = true;
        while first || pos < chars.len() {
            if !cur.is_empty() && cap - cur.len() < 2 {
                pages.push(std::mem::take(&mut cur));
            }
            let head = if first {
                s.heading.clone()
            } else {
                format!("{}(続き)", s.heading)
            };
            cur.push(Row::Head(tone, head));
            first = false;
            while cur.len() < cap && pos < chars.len() {
                while chars.get(pos) == Some(&' ') {
                    pos += 1;
                }
                if pos >= chars.len() {
                    break;
                }
                let end = line_end(&chars, pos, 0.0, max_w, adv);
                cur.push(Row::Body(chars[pos..end].iter().collect()));
                pos = end;
            }
        }
    }
    if !cur.is_empty() {
        pages.push(cur);
    }
    pages
}

/// 矩形 at (履歴の矩形) を枠にして描く。見出しの帯は、いちばん重い種別 (band.top) の色
pub fn draw(pm: &mut Pixmap, text: &mut Text, band: &QuakeBand, at: Rect, now_ms: u64) {
    let pages = layout(band, at.w - PAD * 2.0, capacity(at.h), &mut |c| {
        text.width(c.encode_utf8(&mut [0; 4]), ROW_PX)
    });
    let page = page_at(now_ms, pages.len());
    // 右パネルの左の縁 (1px) は残す。地の色 (PANEL) は動かない部分がすでに塗っている
    let (x, w) = (at.x + 1.0, at.w - 1.0);
    rect(pm, x, at.y, w, TITLE_H, banner_color(band.top.level()), 1.0);
    if band.top == Tone::Special {
        rect(pm, x, at.y, w, 1.0, WHITE, 1.0);
        rect(pm, x, at.y + TITLE_H - 1.0, w, 1.0, WHITE, 1.0);
    }
    text.draw(
        pm,
        TITLE,
        at.x + PAD,
        at.y + TITLE_H / 2.0 + TITLE_PX * 0.35,
        TITLE_PX,
        WHITE,
    );
    if pages.len() > 1 {
        let label = format!("{}/{}", page + 1, pages.len());
        text.draw_right(pm, &label, at.right() - PAD, at.y + TITLE_H / 2.0 + 4.0, 12.0, WHITE);
    }
    for (i, row) in pages[page].iter().enumerate() {
        let y = at.y + TITLE_H + 2.0 + ROW_H * i as f32;
        let base = y + ROW_H / 2.0 + ROW_PX * 0.35;
        match row {
            Row::Head(tone, s) => {
                rect(pm, x, y, w, ROW_H, banner_color(tone.level()), 1.0);
                text.draw(pm, s, at.x + PAD, base, ROW_PX, WHITE);
            }
            Row::Body(s) => {
                text.draw(pm, s, at.x + PAD, base, ROW_PX, TEXT);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broadcast::native::banner::Section;

    fn adv(_: char) -> f32 {
        1.0
    }

    fn band(items: &[(Tone, &str, &str)]) -> QuakeBand {
        QuakeBand {
            top: items[0].0,
            sections: items.iter().map(|i| Section::new(i.1, i.2.to_string())).collect(),
            tones: items.iter().map(|i| i.0).collect(),
        }
    }

    fn head(t: Tone, s: &str) -> Row {
        Row::Head(t, s.into())
    }
    fn body(s: &str) -> Row {
        Row::Body(s.into())
    }

    #[test]
    fn one_kind_is_a_heading_and_its_areas() {
        let b = band(&[(Tone::Tsunami, "【津波警報】", "宮城県・岩手県")]);
        assert_eq!(
            layout(&b, 20.0, 5, &mut adv),
            vec![vec![head(Tone::Tsunami, "【津波警報】"), body("宮城県・岩手県")]]
        );
    }

    #[test]
    fn a_long_list_wraps_after_a_separator_and_keeps_every_character() {
        let b = band(&[(Tone::Tsunami, "【津波警報】", "あいう・えおか・きくけ")]);
        let p = layout(&b, 6.0, 9, &mut adv);
        assert_eq!(
            p,
            vec![vec![
                head(Tone::Tsunami, "【津波警報】"),
                body("あいう・"),
                body("えおか・"),
                body("きくけ"),
            ]]
        );
    }

    #[test]
    fn kinds_follow_each_other_on_a_page_and_keep_their_tones() {
        let b = band(&[
            (Tone::MajorTsunami, "【大津波警報】", "宮城県"),
            (Tone::Special, "【特別警報】", "大雨特別警報"),
        ]);
        let p = layout(&b, 20.0, 4, &mut adv);
        assert_eq!(
            p,
            vec![vec![
                head(Tone::MajorTsunami, "【大津波警報】"),
                body("宮城県"),
                head(Tone::Special, "【特別警報】"),
                body("大雨特別警報"),
            ]]
        );
    }

    #[test]
    fn a_heading_never_ends_a_page_alone() {
        // 1 種別目で 3 行のうち 2 行を使うと、2 種別目の見出しだけが残る空き 1 行 -> 次のページから
        let b = band(&[
            (Tone::MajorTsunami, "【大津波警報】", "宮城県"),
            (Tone::Tsunami, "【津波警報】", "岩手県"),
        ]);
        let p = layout(&b, 20.0, 3, &mut adv);
        assert_eq!(p.len(), 2);
        assert_eq!(p[1], vec![head(Tone::Tsunami, "【津波警報】"), body("岩手県")]);
    }

    #[test]
    fn a_kind_continued_on_the_next_page_repeats_its_heading() {
        let b = band(&[(Tone::Tsunami, "【津波警報】", "あい・うえ・おか・きく")]);
        let p = layout(&b, 3.0, 3, &mut adv);
        assert_eq!(
            p,
            vec![
                vec![head(Tone::Tsunami, "【津波警報】"), body("あい・"), body("うえ・")],
                vec![head(Tone::Tsunami, "【津波警報】(続き)"), body("おか・"), body("きく")],
            ]
        );
    }

    #[test]
    fn the_small_panel_still_shows_one_heading_and_one_row() {
        // 地震の画面の履歴の矩形 (約 90px)
        assert!(capacity(90.0) >= 2);
        assert_eq!(capacity(0.0), 2);
        assert_eq!(capacity(262.0), 11);
        let b = band(&[(Tone::Tsunami, "【津波警報】", "宮城県")]);
        assert_eq!(layout(&b, 20.0, 0, &mut adv).len(), 1);
    }
}
