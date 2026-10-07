//! 平時の気象警報の県ごとの一覧 (docs/superpowers/specs/2026-10-07-pref-warning-list.md)。
//! 警報 (特別警報を含む。注意報は含めない) が 1 つでもあるあいだ、お知らせの矩形を一覧に置き換える。
//! 告知があれば一覧のページ送りの最後に告知のページを 1 枚挟む。上の帯 (banner.rs) は、ここの要約文を出す。
//! 集計・並び・要約・ページ割りは純粋な関数、描画は draw。

use std::collections::{BTreeMap, BTreeSet};

use tiny_skia::Pixmap;

use super::banner::{banner_color, page_at};
use super::data::{pref_code, pref_of, warning_level, WarningLevel, Warnings};
use super::layout_resolve::Rect;
use super::notice::{self, LayoutCache, Notices};
use super::paint::{rect, rrect, BG, LINE, MUTED, TEXT};
use super::text::Text;

/// 帯の要約に出す県の数 (これを超えると「ほかN県」)
const SUMMARY_PREFS: usize = 6;
/// 1 ページの県の数
const PER_PAGE: usize = 5;
const SIDE_PAD: f32 = 16.0;
const PAD_X: f32 = 12.0;
const PAD_Y: f32 = 7.0;
const ROW_H: f32 = 20.0;
const ROW_PX: f32 = 13.0;
const NAME_W: f32 = 60.0;
const COUNT_W: f32 = 64.0;
const LABEL_PX: f32 = 12.0;
const WHITE: [u8; 3] = [0xff, 0xff, 0xff];
/// 警報の種類を並べる順 (同じ段階のなか)。ここに無いものは後ろ
const KIND_ORDER: [&str; 7] = ["大雨", "洪水", "暴風", "暴風雪", "大雪", "波浪", "高潮"];

/// 一覧の 1 行 (1 県)
#[derive(Debug, Clone, PartialEq)]
pub struct PrefRow {
    pub pref: &'static str,
    /// 警報の種類の短い名前 (「大雨特別」「洪水」)。重い順
    pub kinds: Vec<String>,
    /// 警報以上の区域の数
    pub count: usize,
    pub top: WarningLevel,
}

/// 警報の名前を (語幹, 短い表記, 段階) にする。
/// 「レベル３大雨警報」->「大雨」、「大雨特別警報」->「大雨特別」、「レベル４土砂災害危険警報」->「土砂災害危険」
fn short_kind(name: &str) -> (String, String, WarningLevel) {
    let level = warning_level(name);
    let s = name.strip_prefix("レベル").map_or(name, |r| {
        let mut c = r.chars();
        c.next();
        c.as_str()
    });
    let shown = s.strip_suffix("警報").unwrap_or(s);
    let stem = shown
        .strip_suffix("特別")
        .or_else(|| shown.strip_suffix("危険"))
        .unwrap_or(shown);
    (stem.to_string(), shown.to_string(), level)
}

/// 県ごとの警報 (注意報は除く)。北から (都道府県コード順)
pub fn pref_rows(w: &Warnings) -> Vec<PrefRow> {
    // 語幹 -> (段階, 表記)
    type Kinds = BTreeMap<String, (WarningLevel, String)>;
    let mut by_pref: BTreeMap<usize, (usize, Kinds)> = BTreeMap::new();
    for (code, kinds) in &w.areas {
        let Some(p) = pref_code(code) else { continue };
        let mut hit = false;
        for k in kinds {
            let (stem, shown, level) = short_kind(&k.name);
            if level == WarningLevel::Advisory {
                continue;
            }
            hit = true;
            let (_, ks) = by_pref.entry(p).or_default();
            // 同じ語幹は重い方だけ (「大雨」と「大雨特別」が両方あれば後者)
            if ks.get(&stem).is_none_or(|(l, _)| level > *l) {
                ks.insert(stem, (level, shown));
            }
        }
        if hit {
            by_pref.entry(p).or_default().0 += 1;
        }
    }
    by_pref
        .into_iter()
        .map(|(p, (count, ks))| {
            let mut v: Vec<(String, WarningLevel, String)> = ks.into_iter().map(|(s, (l, t))| (s, l, t)).collect();
            v.sort_by_key(|(s, l, _)| {
                let ord = KIND_ORDER.iter().position(|k| k == s).unwrap_or(KIND_ORDER.len());
                (std::cmp::Reverse(*l), ord)
            });
            PrefRow {
                pref: pref_of(&format!("{p:02}")),
                top: v.first().map_or(WarningLevel::Warning, |x| x.1),
                kinds: v.into_iter().map(|x| x.2).collect(),
                count,
            }
        })
        .collect()
}

/// 注意報がある県の数 (警報の有無とは関係なく数える)
pub fn advisory_prefs(w: &Warnings) -> usize {
    w.areas
        .iter()
        .filter(|(_, ks)| ks.iter().any(|k| warning_level(&k.name) == WarningLevel::Advisory))
        .filter_map(|(c, _)| pref_code(c))
        .collect::<BTreeSet<_>>()
        .len()
}

/// 帯での県名。「県」「府」は省く (東京都・北海道は自然な形のまま)
fn short_pref(p: &str) -> &str {
    p.strip_suffix('県').or_else(|| p.strip_suffix('府')).unwrap_or(p)
}

/// 上の帯の要約 (「岩手・宮城・福島 ほか3県 (大雨・洪水・暴風)」)。
/// list は右の一覧が出ているか (出ていれば「→ 右の一覧」を足す)
pub fn summary_text(rows: &[PrefRow], list: bool) -> String {
    let names: Vec<&str> = rows.iter().take(SUMMARY_PREFS).map(|r| short_pref(r.pref)).collect();
    let mut s = names.join("・");
    if let Some(n) = rows.len().checked_sub(SUMMARY_PREFS).filter(|n| *n > 0) {
        s.push_str(&format!(" ほか{n}県"));
    }
    let mut kinds: Vec<&str> = Vec::new();
    for k in rows.iter().flat_map(|r| &r.kinds) {
        if !kinds.contains(&k.as_str()) {
            kinds.push(k);
        }
    }
    s.push_str(&format!(" ({})", kinds.join("・")));
    if list {
        s.push_str(" → 右の一覧");
    }
    s
}

/// 平時の帯の内容
#[derive(Debug, PartialEq)]
pub enum Band {
    /// 警報なし (落ち着いた色の 1 行)
    Quiet(String),
    /// 警報あり (最も重い段階と、「【気象警報】」に続ける要約)
    Alert(WarningLevel, String),
}

pub fn band(w: &Warnings, list: bool) -> Band {
    let rows = pref_rows(w);
    if let Some(top) = rows.iter().map(|r| r.top).max() {
        return Band::Alert(top, summary_text(&rows, list));
    }
    Band::Quiet(match advisory_prefs(w) {
        0 => "気象警報・注意報はありません".to_string(),
        n => format!("気象警報はありません (注意報: {n} 都道府県)"),
    })
}

/// 出すページの種類
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Page {
    /// 県の行の n ページ目 (0 から)
    List(usize),
    Notice,
}

/// 一覧のページと、告知があれば最後に告知のページ
pub fn pages(rows: usize, has_notice: bool) -> Vec<Page> {
    (0..rows.div_ceil(PER_PAGE))
        .map(Page::List)
        .chain(has_notice.then_some(Page::Notice))
        .collect()
}

/// 「大雨・洪水」を幅 max_w に収める (入らなければ「…」)
fn kinds_text(row: &PrefRow, max_w: f32, adv: &mut impl FnMut(char) -> f32) -> String {
    notice::layout(&row.kinds.join("・"), max_w, 1, adv)
        .into_iter()
        .next()
        .unwrap_or_default()
}

/// 一覧 (または告知のページ) を矩形 area (定義の notice) に描く。rows は空でない。area の高さは notice::BOX_MAX_H 以上
pub fn draw(
    pm: &mut Pixmap,
    text: &mut Text,
    cache: &mut LayoutCache,
    rows: &[PrefRow],
    n: Option<&Notices>,
    now_ms: u64,
    area: Rect,
) {
    let has_notice = n.is_some_and(|n| !n.texts.is_empty());
    let all = pages(rows.len(), has_notice);
    let page = page_at(now_ms, all.len());
    match all[page] {
        Page::Notice => {
            if let Some(n) = n {
                notice::draw(pm, text, cache, n, now_ms, area);
            }
        }
        Page::List(i) => draw_list(pm, text, &rows[i * PER_PAGE..rows.len().min((i + 1) * PER_PAGE)], area),
    }
    if all.len() > 1 {
        let label = format!("{}/{}", page + 1, all.len());
        let y = area.y + notice::BOX_MAX_H + LABEL_PX + 2.0;
        text.draw_right(pm, &label, area.right() - SIDE_PAD, y, LABEL_PX, MUTED);
    }
}

fn draw_list(pm: &mut Pixmap, text: &mut Text, rows: &[PrefRow], area: Rect) {
    let (bx, bw) = (area.x + SIDE_PAD, area.w - SIDE_PAD * 2.0);
    let h = notice::BOX_MAX_H;
    rrect(pm, bx, area.y, bw, h, 8.0, LINE, 1.0);
    rrect(pm, bx + 1.0, area.y + 1.0, bw - 2.0, h - 2.0, 7.0, BG, 1.0);
    let (x0, x1) = (bx + PAD_X, bx + bw - PAD_X);
    for (i, r) in rows.iter().enumerate() {
        let y = area.y + PAD_Y + ROW_H * i as f32;
        let base = y + ROW_H / 2.0 + ROW_PX * 0.35;
        let special = r.top == WarningLevel::Emergency;
        let fg = if special { WHITE } else { TEXT };
        // 段階の色: 特別警報は帯と同じ黒地に白い縁の行、ほかは左の細い印
        if special {
            rect(pm, bx + 2.0, y, bw - 4.0, ROW_H, banner_color(r.top), 1.0);
            rect(pm, bx + 2.0, y, bw - 4.0, 1.0, WHITE, 1.0);
            rect(pm, bx + 2.0, y + ROW_H - 1.0, bw - 4.0, 1.0, WHITE, 1.0);
        } else {
            rect(pm, bx + 3.0, y + 3.0, 4.0, ROW_H - 6.0, banner_color(r.top), 1.0);
        }
        text.draw(pm, r.pref, x0 + 2.0, base, ROW_PX, fg);
        let kinds_w = x1 - COUNT_W - (x0 + NAME_W) - 6.0;
        let kinds = kinds_text(r, kinds_w, &mut |c| text.width(c.encode_utf8(&mut [0; 4]), ROW_PX));
        text.draw(pm, &kinds, x0 + NAME_W, base, ROW_PX, fg);
        let count = format!("{} 市町村", r.count);
        text.draw_right(pm, &count, x1, base, ROW_PX, if special { WHITE } else { MUTED });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broadcast::native::data::Kind;

    /// (コード, 警報名...) の並びから Warnings
    fn w(items: &[(&str, &[&str])]) -> Warnings {
        let mut w = Warnings::default();
        for (c, ks) in items {
            w.areas
                .insert(c.to_string(), ks.iter().map(|k| Kind { name: k.to_string() }).collect());
        }
        w
    }

    #[test]
    fn rows_are_per_prefecture_from_the_north_and_advisories_are_left_out() {
        let r = pref_rows(&w(&[
            ("0310100", &["レベル３大雨警報", "レベル２洪水注意報"]),
            ("0310200", &["レベル３洪水警報"]),
            ("0110000", &["レベル３暴風警報"]),
            ("0410000", &["レベル２大雨注意報"]),
        ]));
        assert_eq!(r.len(), 2);
        assert_eq!(
            (r[0].pref, r[0].kinds.clone(), r[0].count),
            ("北海道", vec!["暴風".to_string()], 1)
        );
        assert_eq!(
            (r[1].pref, r[1].kinds.clone(), r[1].count),
            ("岩手県", vec!["大雨".to_string(), "洪水".to_string()], 2)
        );
        assert_eq!(r[1].top, WarningLevel::Warning);
    }

    #[test]
    fn special_comes_first_and_replaces_the_plain_kind_of_the_same_stem() {
        let r = pref_rows(&w(&[
            ("0410000", &["レベル３洪水警報", "大雨特別警報"]),
            ("0410100", &["レベル３大雨警報", "レベル４土砂災害危険警報"]),
        ]));
        assert_eq!(r[0].kinds, ["大雨特別", "土砂災害危険", "洪水"]);
        assert_eq!((r[0].top, r[0].count), (WarningLevel::Emergency, 2));
    }

    #[test]
    fn counting_advisory_prefectures_ignores_warnings_in_other_prefectures() {
        let x = w(&[
            ("0310100", &["レベル２大雨注意報"]),
            ("0310200", &["レベル２強風注意報"]),
            ("0410000", &["レベル２大雨注意報"]),
            ("0510000", &["レベル３大雨警報"]),
        ]);
        assert_eq!(advisory_prefs(&x), 2);
    }

    fn rows_of(n: usize) -> Vec<PrefRow> {
        (1..=n)
            .map(|p| PrefRow {
                pref: pref_of(&format!("{p:02}")),
                kinds: vec!["大雨".into(), "洪水".into()],
                count: 1,
                top: WarningLevel::Warning,
            })
            .collect()
    }

    #[test]
    fn the_summary_names_up_to_six_prefectures_then_counts_the_rest() {
        assert_eq!(summary_text(&rows_of(3)[1..], false), "青森・岩手 (大雨・洪水)");
        let mut r = rows_of(9);
        r[0].kinds = vec!["暴風".into()];
        assert_eq!(
            summary_text(&r, true),
            "北海道・青森・岩手・宮城・秋田・山形 ほか3県 (暴風・大雨・洪水) → 右の一覧"
        );
        // 都は残る
        let tokyo = PrefRow {
            pref: "東京都",
            ..rows_of(1).remove(0)
        };
        assert!(summary_text(&[tokyo], false).starts_with("東京都 "));
    }

    #[test]
    fn the_band_is_a_summary_a_count_of_advisories_or_nothing() {
        let both = w(&[("0310100", &["レベル３大雨警報"]), ("0410100", &["レベル２大雨注意報"])]);
        assert_eq!(
            band(&both, true),
            Band::Alert(WarningLevel::Warning, "岩手 (大雨) → 右の一覧".into())
        );
        let special = w(&[("0310100", &["大雨特別警報"])]);
        assert!(matches!(band(&special, false), Band::Alert(WarningLevel::Emergency, _)));
        let adv = w(&[
            ("0310100", &["レベル２大雨注意報"]),
            ("0410100", &["レベル２大雨注意報"]),
        ]);
        assert_eq!(
            band(&adv, true),
            Band::Quiet("気象警報はありません (注意報: 2 都道府県)".into())
        );
        assert_eq!(
            band(&Warnings::default(), true),
            Band::Quiet("気象警報・注意報はありません".into())
        );
    }

    #[test]
    fn pages_are_five_prefectures_each_with_the_notice_last() {
        assert_eq!(pages(5, false), [Page::List(0)]);
        assert_eq!(pages(6, false), [Page::List(0), Page::List(1)]);
        assert_eq!(pages(2, true), [Page::List(0), Page::Notice]);
        assert_eq!(
            pages(11, true),
            [Page::List(0), Page::List(1), Page::List(2), Page::Notice]
        );
    }

    #[test]
    fn long_kinds_are_cut_with_an_ellipsis_inside_the_width() {
        let r = PrefRow {
            kinds: vec!["大雨特別".into(), "洪水".into(), "暴風".into(), "高潮".into()],
            ..rows_of(1).remove(0)
        };
        let t = kinds_text(&r, 7.0, &mut |_| 1.0);
        assert!(t.ends_with('…') && t.chars().count() <= 7, "{t}");
        assert_eq!(kinds_text(&r, 30.0, &mut |_| 1.0), "大雨特別・洪水・暴風・高潮");
    }
}
