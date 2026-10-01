//! 同じ地震の緊急地震速報の予想を、報が変わっても最大で持ち続ける (web の heldForecast)。
//! 報ごとに予想の地域が出たり空になったりしても、表示は点滅させない。

use crate::quake::{Eew, Scale};

/// 同じ地震の取り消しでない報から持ち続ける値
#[derive(Debug, Clone, PartialEq)]
pub struct Held {
    /// 一度でも警報の報があれば警報
    pub warning: bool,
    pub max_scale: Scale,
    /// 県ごとの予想震度 (全報の最大。最初に出た順)
    pub pref_scales: Vec<(String, Scale)>,
    /// 地域ごとの予想震度 (報ごとの scale_to ?? scale_from の最大。最初に出た順)
    pub area_scales: Vec<(String, Scale)>,
}

/// 名前ごとに最大を取る (最初に出た順を保つ)
fn fold_max<'a>(items: impl Iterator<Item = (&'a str, Scale)>) -> Vec<(String, Scale)> {
    let mut out: Vec<(String, Scale)> = Vec::new();
    for (name, scale) in items {
        match out.iter_mut().find(|(n, _)| n == name) {
            Some((_, s)) => *s = (*s).max(scale),
            None => out.push((name.to_string(), scale)),
        }
    }
    out
}

/// 同じ地震の取り消しでない報 (届いた順は問わない) から、持ち続ける予想を作る
pub fn hold(reports: &[&Eew]) -> Held {
    let areas = reports
        .iter()
        .flat_map(|e| &e.areas)
        .map(|a| (a.name.as_str(), a.scale_to.unwrap_or(a.scale_from).max(a.scale_from)));
    let prefs = reports
        .iter()
        .flat_map(|e| &e.pref_max)
        .map(|p| (p.pref.as_str(), p.scale));
    Held {
        warning: reports.iter().any(|e| e.warning),
        max_scale: reports.iter().map(|e| e.max_scale).max().unwrap_or(Scale::UNKNOWN),
        pref_scales: fold_max(prefs),
        area_scales: fold_max(areas),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quake::{EewArea, PrefScale};

    fn area(name: &str, from: Scale, to: Option<Scale>) -> EewArea {
        EewArea {
            pref: String::new(),
            name: name.into(),
            scale_from: from,
            scale_to: to,
            arrival_time: None,
            arrived: false,
        }
    }

    fn report(serial: u32, max: Scale, areas: Vec<EewArea>, prefs: &[(&str, Scale)], warning: bool) -> Eew {
        Eew {
            event_id: "a".into(),
            serial: serial.to_string(),
            cancelled: false,
            test: false,
            warning,
            issued_at: String::new(),
            origin_time: None,
            origin_time_ms: None,
            hypocenter: None,
            areas,
            pref_max: prefs
                .iter()
                .map(|(p, s)| PrefScale {
                    pref: p.to_string(),
                    scale: *s,
                })
                .collect(),
            max_scale: max,
        }
    }

    fn named(h: &[(String, Scale)]) -> Vec<(&str, i32)> {
        let mut v: Vec<_> = h.iter().map(|(n, s)| (n.as_str(), s.0)).collect();
        v.sort_unstable();
        v
    }

    /// 2026-10-01 21:27 千葉県北東部: 第 5 報で地域が出て、第 6・7 報で空になり、第 8 報でまた出て、第 10 報でまた空になる
    #[test]
    fn areas_and_prefs_stay_at_their_maximum_after_reports_that_omit_them() {
        let both = || {
            vec![
                area("千葉県北東部", Scale::S4, Some(Scale::S4)),
                area("茨城県南部", Scale::S3, Some(Scale::S4)),
            ]
        };
        let prefs = [("千葉県", Scale::S4), ("茨城県", Scale::S4)];
        let seq = [
            report(1, Scale::S3, vec![], &[], false),
            report(5, Scale::S4, both(), &prefs, false),
            report(6, Scale::S3, vec![], &[], false),
            report(7, Scale::S3, vec![], &[], false),
            report(8, Scale::S4, both(), &prefs, false),
            report(
                9,
                Scale::S4,
                vec![area("千葉県北東部", Scale::S4, Some(Scale::S4))],
                &prefs[..1],
                false,
            ),
            report(10, Scale::S3, vec![], &[], false),
        ];
        for n in [3, 4, 7] {
            let r: Vec<&Eew> = seq[..n].iter().collect();
            let h = hold(&r);
            assert_eq!(h.max_scale, Scale::S4, "{n}");
            assert_eq!(named(&h.area_scales), [("千葉県北東部", 40), ("茨城県南部", 40)], "{n}");
            assert_eq!(named(&h.pref_scales), [("千葉県", 40), ("茨城県", 40)], "{n}");
        }
        let first: Vec<&Eew> = seq[..1].iter().collect();
        let h = hold(&first);
        assert!(h.area_scales.is_empty() && h.pref_scales.is_empty());
        assert_eq!(h.max_scale, Scale::S3);
    }

    #[test]
    fn an_open_ended_bound_does_not_erase_a_larger_value_and_the_order_of_arrival_does_not_matter() {
        let a = report(
            1,
            Scale::S4,
            vec![area("茨城県南部", Scale::S3, Some(Scale::S4))],
            &[],
            false,
        );
        let b = report(2, Scale::S3, vec![area("茨城県南部", Scale::S3, None)], &[], false);
        for r in [[&a, &b], [&b, &a]] {
            assert_eq!(named(&hold(&r).area_scales), [("茨城県南部", 40)]);
        }
    }

    #[test]
    fn a_warning_stays_a_warning() {
        let w = report(1, Scale::S5_LOWER, vec![], &[], true);
        let f = report(2, Scale::S3, vec![], &[], false);
        assert!(hold(&[&w, &f]).warning);
        assert!(!hold(&[&f]).warning);
    }
}
