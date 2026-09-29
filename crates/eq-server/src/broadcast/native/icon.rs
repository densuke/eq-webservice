//! 天気のアイコン (気象庁の天気予報の SVG)。天気コード -> ファイル名は telops.rs の表、
//! SVG は実行時に取って resvg で Pixmap にし、名前ごとに覚える (描き直しのたびに SVG を描かない)。
//! 取れない・描けないときは、呼ぶ側が漢字 1 文字に戻す。

use std::collections::HashMap;

use tiny_skia::{Pixmap, Transform};

use super::telops::TELOPS;

/// SVG の取得先 (この下に <名前> が続く)
pub const IMG_BASE: &str = "https://www.jma.go.jp/bosai/forecast/img/";
/// 札の中のアイコンの高さ (px)
pub const ICON_H: u32 = 18;

/// 覚えているアイコン (キーは SVG のファイル名)
pub type Icons = HashMap<String, Pixmap>;

/// 天気コード -> (昼のファイル名, 夜のファイル名)
pub fn names(code: &str) -> Option<(&'static str, &'static str)> {
    let code: u16 = code.parse().ok()?;
    let i = TELOPS.binary_search_by_key(&code, |e| e.0).ok()?;
    Some((TELOPS[i].1, TELOPS[i].2))
}

/// 夜 (18 時〜翌 6 時、JST) か (web/src/weather-layer.ts と同じ)
pub fn is_night(now_ms: u64) -> bool {
    let hour = (now_ms / 3_600_000 + 9) % 24;
    !(6..18).contains(&hour)
}

/// その時間に出すアイコンのファイル名
pub fn name_for(code: &str, now_ms: u64) -> Option<&'static str> {
    names(code).map(|(day, night)| if is_night(now_ms) { night } else { day })
}

/// SVG を高さ ICON_H の Pixmap にする (縦横比は保つ)
pub fn rasterize(svg: &[u8]) -> Option<Pixmap> {
    let tree = resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default()).ok()?;
    let size = tree.size();
    let k = ICON_H as f32 / size.height();
    let mut pm = Pixmap::new((size.width() * k).round().max(1.0) as u32, ICON_H)?;
    resvg::render(&tree, Transform::from_scale(k, k), &mut pm.as_mut());
    Some(pm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_map_to_day_and_night_icons() {
        assert_eq!(names("100"), Some(("100.svg", "500.svg")));
        assert_eq!(names("200"), Some(("200.svg", "200.svg")));
        assert_eq!(names("302"), Some(("302.svg", "302.svg")));
        assert_eq!(names("999"), None);
        assert_eq!(names(""), None);
        assert_eq!(names("abc"), None);
    }

    #[test]
    fn the_table_is_sorted_for_binary_search() {
        assert!(TELOPS.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(TELOPS.len(), 118);
    }

    #[test]
    fn night_is_18_to_6_in_jst() {
        // 2026-09-30 00:00 JST = 09-29 15:00 UTC
        let jst0 = 1_790_000_000_000 / 86_400_000 * 86_400_000 - 9 * 3_600_000 + 86_400_000;
        let at = |h: u64| jst0 + h * 3_600_000;
        assert!(is_night(at(0)) && is_night(at(5)) && is_night(at(18)) && is_night(at(23)));
        assert!(!is_night(at(6)) && !is_night(at(12)) && !is_night(at(17)));
        assert_eq!(name_for("100", at(12)), Some("100.svg"));
        assert_eq!(name_for("100", at(20)), Some("500.svg"));
        assert_eq!(name_for("xyz", at(12)), None);
    }

    #[test]
    fn svg_is_drawn_at_the_icon_height() {
        let svg = include_bytes!("testdata/sun.svg");
        let pm = rasterize(svg).unwrap();
        assert_eq!((pm.width(), pm.height()), (27, ICON_H)); // 90x60 -> 27x18
        assert!(pm.pixels().iter().any(|p| p.alpha() > 0));
        assert!(rasterize(b"not svg").is_none());
    }
}
