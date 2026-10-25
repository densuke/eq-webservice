//! 平時に危険警報以上が出ている県へ、毎分の終わりに地図を寄せる (web/src/warnings.ts の warnFocuses・mergeFocuses・warnFocusAt と同じ規則)。
//! 全国の地図だと小さな区域が埋もれるため。寄せている間は札と矢印で種類と地域を示す

use std::collections::BTreeMap;

use super::camera::{MapBox, KM_TO_UNITS};
use super::data::{pref_of, warning_items, warning_level, Kind, WarnItem, WarningLevel, Warnings};

/// 寄せる先 (1 県、または 1 画面に収まる隣り合う数県)。items はその県の危険警報以上だけ
#[derive(Debug, Clone, PartialEq)]
pub struct WarnFocus {
    pub prefs: Vec<&'static str>,
    pub codes: Vec<String>,
    pub items: Vec<WarnItem>,
    pub top: WarningLevel,
}

/// この数の県に危険警報以上が出ていたら寄せない (全国の地図のほうが分かり、巡回も長くなりすぎる)
const MAX_FOCUS_PREFS: usize = 6;
/// 1 分のうち寄せる秒数 (毎分の後ろ)
const FOCUS_SEC: u64 = 15;
/// まとめて寄せる県の範囲の上限 (km 四方)
pub const MERGE_KM: f64 = 600.0;

fn focused(k: &Kind) -> bool {
    warning_level(&k.name) >= WarningLevel::Danger
}

fn focus_of(
    prefs: Vec<&'static str>,
    areas: BTreeMap<String, Vec<Kind>>,
    names: &BTreeMap<String, String>,
) -> WarnFocus {
    let w = Warnings {
        areas,
        names: names.clone(),
    };
    let items = warning_items(&w);
    WarnFocus {
        prefs,
        codes: w.areas.into_keys().collect(),
        top: items[0].level,
        items,
    }
}

/// 危険警報以上の区域を県ごとにまとめる。特別警報の県があればその県だけ。段階の高い県から、同じなら県の番号順
pub fn warn_focuses(w: &Warnings) -> Vec<WarnFocus> {
    let mut by_pref: Vec<(&'static str, BTreeMap<String, Vec<Kind>>)> = Vec::new();
    for (code, kinds) in &w.areas {
        let kinds: Vec<Kind> = kinds.iter().filter(|k| focused(k)).cloned().collect();
        if kinds.is_empty() {
            continue;
        }
        let pref = pref_of(code);
        match by_pref.iter_mut().find(|(p, _)| *p == pref) {
            Some((_, areas)) => {
                areas.insert(code.clone(), kinds);
            }
            None => by_pref.push((pref, BTreeMap::from([(code.clone(), kinds)]))),
        }
    }
    let mut all: Vec<WarnFocus> = by_pref
        .into_iter()
        .map(|(pref, areas)| focus_of(vec![pref], areas, &w.names))
        .collect();
    all.sort_by_key(|f| std::cmp::Reverse(f.top));
    if all.iter().any(|f| f.top == WarningLevel::Emergency) {
        all.retain(|f| f.top == WarningLevel::Emergency);
    }
    if all.len() >= MAX_FOCUS_PREFS {
        return Vec::new();
    }
    all
}

/// 1 画面 (幅・高さ max_km 以下) に収まる県どうしを、段階の高い順にまとめる。
/// box_of は区域のコードの外接矩形 (地図の座標)。区域が分からなければ None で、まとめない
pub fn merge_focuses(
    fs: Vec<WarnFocus>,
    w: &Warnings,
    box_of: impl Fn(&[String]) -> Option<MapBox>,
    max_km: f64,
) -> Vec<WarnFocus> {
    let max = max_km * KM_TO_UNITS;
    let fits = |codes: &[String]| box_of(codes).is_some_and(|b| b.x1 - b.x0 <= max && b.y1 - b.y0 <= max);
    let mut groups: Vec<Vec<WarnFocus>> = Vec::new();
    for f in fs {
        let at = groups.iter().position(|g| {
            let codes: Vec<String> = g.iter().chain([&f]).flat_map(|x| x.codes.iter().cloned()).collect();
            fits(&codes)
        });
        match at {
            Some(i) => groups[i].push(f),
            None => groups.push(vec![f]),
        }
    }
    groups
        .into_iter()
        .map(|mut g| {
            if g.len() == 1 {
                return g.remove(0);
            }
            let areas = g
                .iter()
                .flat_map(|f| &f.codes)
                .map(|c| (c.clone(), w.areas[c].iter().filter(|k| focused(k)).cloned().collect()))
                .collect();
            focus_of(
                g.iter().flat_map(|f| f.prefs.iter().copied()).collect(),
                areas,
                &w.names,
            )
        })
        .collect()
}

/// 今寄せる先。毎分の後ろ FOCUS_SEC 秒だけ、1 分ごとに次の先へ
pub fn focus_at(fs: &[WarnFocus], now_ms: u64) -> Option<&WarnFocus> {
    if fs.is_empty() || (now_ms / 1000) % 60 < 60 - FOCUS_SEC {
        return None;
    }
    fs.get((now_ms / 60_000) as usize % fs.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(name: &str) -> Vec<Kind> {
        vec![Kind { name: name.into() }]
    }

    fn warnings(areas: &[(&str, &str)], names: &[(&str, &str)]) -> Warnings {
        Warnings {
            areas: areas.iter().map(|(c, k)| (c.to_string(), kind(k))).collect(),
            names: names.iter().map(|(c, n)| (c.to_string(), n.to_string())).collect(),
        }
    }

    #[test]
    fn only_danger_and_emergency_areas_are_focused_one_prefecture_at_a_time() {
        let mut w = warnings(
            &[
                ("3036100", "レベル４土砂災害危険警報"),
                ("2720000", "レベル３大雨警報"),
                ("2920100", "レベル４土砂災害危険警報"),
            ],
            &[("3020500", "有田市"), ("3036100", "有田川町"), ("2920100", "奈良市")],
        );
        w.areas.insert(
            "3020500".into(),
            vec![
                Kind {
                    name: "レベル４土砂災害危険警報".into(),
                },
                Kind {
                    name: "レベル２大雨注意報".into(),
                },
            ],
        );
        let fs = warn_focuses(&w);
        let got: Vec<_> = fs.iter().map(|f| (f.prefs.clone(), f.codes.clone())).collect();
        assert_eq!(
            got,
            vec![
                (vec!["奈良県"], vec!["2920100".to_string()]),
                (vec!["和歌山県"], vec!["3020500".to_string(), "3036100".to_string()]),
            ]
        );
        assert_eq!(fs[1].items.len(), 1);
        assert_eq!(fs[1].items[0].places(usize::MAX), "和歌山県 有田市・有田川町");
        assert!(warn_focuses(&warnings(&[("2720000", "レベル３大雨警報")], &[])).is_empty());
    }

    #[test]
    fn a_special_warning_narrows_the_focus_to_its_prefectures() {
        let w = warnings(
            &[
                ("3020500", "レベル４土砂災害危険警報"),
                ("4020000", "レベル５大雨特別警報"),
            ],
            &[],
        );
        let fs = warn_focuses(&w);
        assert_eq!(fs.len(), 1);
        assert_eq!(
            (fs[0].prefs.clone(), fs[0].top),
            (vec!["福岡県"], WarningLevel::Emergency)
        );
    }

    #[test]
    fn danger_over_many_prefectures_keeps_the_whole_country() {
        let codes: Vec<String> = (1..=6).map(|n| format!("{n:02}00000")).collect();
        let areas = |n: usize| -> Vec<(&str, &str)> {
            codes[..n]
                .iter()
                .map(|c| (c.as_str(), "レベル４土砂災害危険警報"))
                .collect()
        };
        assert_eq!(warn_focuses(&warnings(&areas(5), &[])).len(), 5);
        assert!(warn_focuses(&warnings(&areas(6), &[])).is_empty());
    }

    #[test]
    fn neighbors_that_fit_one_view_are_shown_together() {
        let w = warnings(
            &[
                ("2920100", "レベル４土砂災害危険警報"),
                ("3020500", "レベル４土砂災害危険警報"),
                ("0110000", "レベル４土砂災害危険警報"),
            ],
            &[("2920100", "奈良市"), ("3020500", "有田市"), ("0110000", "札幌市")],
        );
        let at = |c: &str| match c {
            "2920100" => MapBox {
                x0: 0.0,
                y0: 0.0,
                x1: 10.0,
                y1: 10.0,
            },
            "3020500" => MapBox {
                x0: 20.0,
                y0: 20.0,
                x1: 30.0,
                y1: 30.0,
            },
            _ => MapBox {
                x0: 900.0,
                y0: -900.0,
                x1: 950.0,
                y1: -850.0,
            },
        };
        let box_of = |codes: &[String]| codes.iter().map(|c| Some(at(c))).reduce(MapBox::union).flatten();
        let merged = merge_focuses(warn_focuses(&w), &w, box_of, 100.0);
        let got: Vec<_> = merged.iter().map(|f| f.prefs.clone()).collect();
        assert_eq!(got, vec![vec!["北海道"], vec!["奈良県", "和歌山県"]]);
        assert_eq!(merged[1].items[0].places(usize::MAX), "奈良県 奈良市、和歌山県 有田市");
        // 区域が分からなければまとめない
        assert_eq!(merge_focuses(warn_focuses(&w), &w, |_| None, 100.0).len(), 3);
    }

    #[test]
    fn the_map_zooms_in_for_the_last_part_of_each_minute_visiting_each_in_turn() {
        let w = warnings(
            &[
                ("2920100", "レベル４土砂災害危険警報"),
                ("3020500", "レベル４土砂災害危険警報"),
            ],
            &[],
        );
        let fs = warn_focuses(&w);
        assert!(focus_at(&fs, 30_000).is_none());
        assert_eq!(focus_at(&fs, 50_000).unwrap().prefs, vec!["奈良県"]);
        assert_eq!(focus_at(&fs, 110_000).unwrap().prefs, vec!["和歌山県"]);
        assert_eq!(focus_at(&fs, 170_000).unwrap().prefs, vec!["奈良県"]);
        assert!(focus_at(&[], 50_000).is_none());
    }
}
