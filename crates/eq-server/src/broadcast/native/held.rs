//! 同じ地震の緊急地震速報の予想を、報が変わっても最大で持ち続ける (web の heldForecast)。
//! 報ごとに予想の地域が出たり空になったりしても、表示は点滅させない。

use crate::quake::{Eew, Scale};

/// これより深い地震の緊急地震速報は予測震度を出さない (気象庁。深さ 150km の報には予測震度が付いた例がある)
const DEEP_KM: i32 = 150;

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

/// 最新の報 (serial が最大) が深い地震で予測震度が不明なら、浅いと見ていた頃の予想は捨てる
/// (2026-10-25 小笠原諸島西方沖: 第 1〜3 報の深さ 10km・震度 2 が、深さ 200km 以上で不明になった後も残った)
fn deep_unknown(reports: &[&Eew]) -> bool {
    let latest = reports.iter().max_by_key(|e| e.serial.parse::<u64>().unwrap_or(0));
    latest.is_some_and(|e| {
        !e.max_scale.is_known()
            && e.hypocenter
                .as_ref()
                .and_then(|h| h.depth_km)
                .is_some_and(|d| d > DEEP_KM)
    })
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
        max_scale: if deep_unknown(reports) {
            Scale::UNKNOWN
        } else {
            reports.iter().map(|e| e.max_scale).max().unwrap_or(Scale::UNKNOWN)
        },
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

    fn at_depth(serial: u32, max: Scale, depth_km: i32) -> Eew {
        Eew {
            hypocenter: Some(crate::quake::Hypocenter {
                name: "小笠原諸島西方沖".into(),
                latitude: Some(28.4),
                longitude: Some(140.0),
                depth_km: Some(depth_km),
                magnitude: Some(5.8),
            }),
            ..report(serial, max, vec![], &[], false)
        }
    }

    /// 2026-10-25 小笠原諸島西方沖: 第 1〜3 報は深さ 10km・震度 2、第 4 報から深さ 200km 以上・不明
    #[test]
    fn a_deep_report_without_a_forecast_drops_the_shallow_forecast() {
        let seq = [
            at_depth(1, Scale::S2, 10),
            at_depth(3, Scale::S2, 10),
            at_depth(4, Scale::UNKNOWN, 200),
        ];
        let all: Vec<&Eew> = seq.iter().collect();
        assert_eq!(hold(&all).max_scale, Scale::UNKNOWN);
        assert_eq!(hold(&all[..2]).max_scale, Scale::S2);
        // 届いた順は問わない
        assert_eq!(hold(&[&seq[2], &seq[0]]).max_scale, Scale::UNKNOWN);
        // 深さが浅く戻って予測が出れば、また最大で持つ
        let back = at_depth(5, Scale::S3, 60);
        assert_eq!(hold(&[&seq[0], &seq[2], &back]).max_scale, Scale::S3);
        // 浅い報が不明でも、前の予想は捨てない
        assert_eq!(hold(&[&seq[0], &at_depth(2, Scale::UNKNOWN, 10)]).max_scale, Scale::S2);
    }

    #[test]
    fn a_warning_stays_a_warning() {
        let w = report(1, Scale::S5_LOWER, vec![], &[], true);
        let f = report(2, Scale::S3, vec![], &[], false);
        assert!(hold(&[&w, &f]).warning);
        assert!(!hold(&[&f]).warning);
    }
}
