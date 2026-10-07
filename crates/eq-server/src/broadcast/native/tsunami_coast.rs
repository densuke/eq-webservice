//! 津波予報区の海岸線を、等級の色で本図 (と別枠・寄り・サブの地図) に塗る。見た目は web/src/map.css の .tsunami-line、
//! 色は style.css の --t-watch / --t-warning / --t-major。注意報も塗る (web と同じ)。判断 (どの線をどの色で) は plan。

use tiny_skia::Pixmap;

use super::frame::Frame;
use super::geo::Coast;
use crate::quake::model::{TsunamiArea, TsunamiGrade};

/// 線の色と太さ (画面の px)。大津波警報は太め
fn style(grade: TsunamiGrade) -> Option<([u8; 3], f32)> {
    match grade {
        TsunamiGrade::Unknown => None,
        TsunamiGrade::Watch => Some(([0xfa, 0xf5, 0x00], 5.0)),
        TsunamiGrade::Warning => Some(([0xff, 0x28, 0x00], 5.0)),
        TsunamiGrade::MajorWarning => Some(([0xc8, 0x00, 0xff], 7.0)),
    }
}

/// 塗る線: (線の添字, 等級)。予報区の名前が一致する線だけ (無い名前は無視)。重い等級が上に重なるよう軽い順
pub fn plan<'a>(names: impl Iterator<Item = &'a str>, areas: &[TsunamiArea]) -> Vec<(usize, TsunamiGrade)> {
    let mut out: Vec<(usize, TsunamiGrade)> = names
        .enumerate()
        .filter_map(|(i, n)| {
            let g = areas.iter().filter(|a| a.name == n).map(|a| a.grade).max()?;
            style(g).map(|_| (i, g))
        })
        .collect();
    out.sort_by_key(|&(_, g)| g);
    out
}

/// その面 f に海岸線を描く (黒い縁取りを敷いてから色)。areas が空なら何もしない
pub fn draw(pm: &mut Pixmap, f: &Frame, coast: &[Coast], areas: &[TsunamiArea]) {
    if areas.is_empty() {
        return;
    }
    for (i, g) in plan(coast.iter().map(|c| c.name.as_str()), areas) {
        let Some((color, w)) = style(g) else { continue };
        f.stroke_round(pm, &coast[i].path, [0, 0, 0], 0.85, w + 3.0);
        f.stroke_round(pm, &coast[i].path, color, 1.0, w);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(name: &str, grade: TsunamiGrade) -> TsunamiArea {
        TsunamiArea {
            name: name.into(),
            grade,
            immediate: false,
            first_height: None,
            max_height: None,
        }
    }

    #[test]
    fn only_named_areas_with_a_grade_are_painted_lightest_first() {
        let names = ["岩手県", "宮城県", "青森県太平洋沿岸", "福島県", "茨城県"];
        let areas = [
            area("宮城県", TsunamiGrade::MajorWarning),
            area("岩手県", TsunamiGrade::Warning),
            area("茨城県", TsunamiGrade::Watch),
            area("福島県", TsunamiGrade::Unknown),       // 予報なしは塗らない
            area("存在しない区", TsunamiGrade::Warning), // 線が無い名前は無視
        ];
        assert_eq!(
            plan(names.into_iter(), &areas),
            vec![
                (4, TsunamiGrade::Watch),
                (0, TsunamiGrade::Warning),
                (1, TsunamiGrade::MajorWarning)
            ]
        );
        assert!(plan(names.into_iter(), &[]).is_empty());
    }

    #[test]
    fn the_colors_and_widths_follow_the_web_css() {
        assert_eq!(style(TsunamiGrade::Watch), Some(([0xfa, 0xf5, 0x00], 5.0)));
        assert_eq!(style(TsunamiGrade::Warning), Some(([0xff, 0x28, 0x00], 5.0)));
        assert_eq!(style(TsunamiGrade::MajorWarning), Some(([0xc8, 0x00, 0xff], 7.0)));
        assert_eq!(style(TsunamiGrade::Unknown), None);
    }

    /// web のデモ (津波) の予報区は、すべて海岸線の線がある名前 (塗り漏れが無い)
    #[test]
    fn every_demo_area_has_a_line_in_the_real_geojson() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/public");
        let coast = std::fs::read_to_string(dir.join("tsunami.geojson")).unwrap();
        let demo = std::fs::read_to_string(dir.join("demo/tsunami.json")).unwrap();
        let view = super::super::geo::View::fit_home((0.0, 36.0, 900.0, 684.0));
        let lines = super::super::geo::parse_coast(&coast, &view).unwrap();
        assert!(lines.len() > 60);
        let v: serde_json::Value = serde_json::from_str(&demo).unwrap();
        let mut n = 0;
        for e in v["events"].as_array().unwrap() {
            if e["kind"] == "tsunami" {
                for a in e["areas"].as_array().unwrap() {
                    let name = a["name"].as_str().unwrap();
                    assert!(lines.iter().any(|l| l.name == name), "{name}");
                    n += 1;
                }
            }
        }
        assert_eq!(n, 7);
    }
}
