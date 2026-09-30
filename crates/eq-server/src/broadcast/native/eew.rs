//! 緊急地震速報と地震波の表示の判断 (純粋な関数。時刻は引数)。model.rs の続き。
//! web/src/quakes.ts (activeEews・waveSources)、waves.ts、detail.ts (forecastTag) と同じ規則にする。

use crate::quake::{Event, EventBody, Hypocenter, Scale};

use crate::broadcast::record::Shown;

use super::model::{current_quake, same_quake, Place, QuakeSummary};

/// 緊急地震速報を表示し続ける時間 (web の EEW_BANNER_MS)
const EEW_ACTIVE_MS: u64 = 3 * 60_000;
/// 地震情報が届いた緊急地震速報を、その地震情報に任せるかを見る範囲 (web の FULL_MS)
const RECENT_MS: u64 = 10 * 60_000;
/// 発生からこの時間を過ぎた地震波は描かない (web の WAVE_MAX_SEC)
const WAVE_MAX_MS: i64 = 180_000;
const VP_KM_S: f64 = 6.5;
const VS_KM_S: f64 = 3.75;
/// 深さが分からないときの深さ (web の geoOf)
const DEFAULT_DEPTH_KM: f64 = 10.0;

/// 同じ地震 (event_id) の最新の報
#[derive(Debug, Clone, PartialEq)]
pub struct EewSummary {
    pub event_id: String,
    pub serial: String,
    /// 最新の報を受けた時刻 (サーバの時計。epoch ミリ秒)
    pub received_ms: u64,
    pub warning: bool,
    pub test: bool,
    /// 発生時刻の文字列 (無ければ発表時刻)
    pub origin_time: String,
    pub origin_ms: Option<i64>,
    pub hypocenter: Option<Hypocenter>,
    pub max_scale: Scale,
    /// 県ごとの予測震度
    pub pref_scales: Vec<(String, Scale)>,
    /// 地域ごとの予測があるか (札を出すかの判断にだけ使う。地域の塗りは細かすぎて描かない)
    pub has_areas: bool,
}

/// 緊急地震速報を同じ地震ごとにまとめる。最新の報 (serial が最大) が取り消しのものは外す
pub fn latest_eews(events: &[Event]) -> Vec<EewSummary> {
    let mut latest: Vec<(&Event, &crate::quake::Eew)> = Vec::new();
    for ev in events {
        let EventBody::Eew(e) = &ev.body else { continue };
        let serial = |x: &crate::quake::Eew| x.serial.parse::<u64>().unwrap_or(0);
        match latest.iter_mut().find(|(_, o)| o.event_id == e.event_id) {
            Some(slot) if serial(e) >= serial(slot.1) => *slot = (ev, e),
            Some(_) => {}
            None => latest.push((ev, e)),
        }
    }
    latest
        .into_iter()
        .filter(|(_, e)| !e.cancelled)
        .map(|(ev, e)| EewSummary {
            event_id: e.event_id.clone(),
            serial: e.serial.clone(),
            received_ms: ev.received_at_ms,
            warning: e.warning,
            test: e.test,
            origin_time: e.origin_time.clone().unwrap_or_else(|| e.issued_at.clone()),
            origin_ms: e.origin_time_ms,
            hypocenter: e.hypocenter.clone(),
            max_scale: e.max_scale,
            pref_scales: e.pref_max.iter().map(|p| (p.pref.clone(), p.scale)).collect(),
            has_areas: !e.areas.is_empty(),
        })
        .collect()
}

fn eew_place(e: &EewSummary) -> Place {
    let h = e.hypocenter.as_ref();
    Place {
        origin_ms: e.origin_ms,
        lat: h.and_then(|h| h.latitude),
        lon: h.and_then(|h| h.longitude),
    }
}

fn quake_place(q: &QuakeSummary) -> Place {
    let h = q.hypocenter.as_ref();
    Place {
        origin_ms: q.origin_ms,
        lat: h.and_then(|h| h.latitude),
        lon: h.and_then(|h| h.longitude),
    }
}

fn is_active(e: &EewSummary, now_ms: u64) -> bool {
    now_ms.saturating_sub(e.received_ms) < EEW_ACTIVE_MS
}

/// 表示中の緊急地震速報 (新しい順)。同じ地震の地震情報が届いたものは、その地震情報に任せる
/// (予測の震度で居座らないように。web の priorityGroups と同じ)
fn shown_eews<'a>(quakes: &[QuakeSummary], eews: &'a [EewSummary], now_ms: u64) -> Vec<&'a EewSummary> {
    let recent: Vec<Place> = quakes
        .iter()
        .filter(|q| now_ms.saturating_sub(q.updated_ms) <= RECENT_MS)
        .map(quake_place)
        .collect();
    let mut out: Vec<_> = eews
        .iter()
        .filter(|e| is_active(e, now_ms) && !recent.iter().any(|p| same_quake(&eew_place(e), p)))
        .collect();
    out.sort_by_key(|e| std::cmp::Reverse((e.max_scale, e.received_ms)));
    out
}

/// 地震の画面に出すもの
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Current<'a> {
    Quake(&'a QuakeSummary),
    Eew(&'a EewSummary),
}

/// 地震の画面に出す地震・緊急地震速報 (無ければ平時)。揺れの大きい方 (緊急地震速報は予測、地震情報は観測)、同じなら新しい方
pub fn current<'a>(quakes: &'a [QuakeSummary], eews: &'a [EewSummary], now_ms: u64) -> Option<Current<'a>> {
    let q = current_quake(quakes, now_ms).map(|q| ((q.max_scale, q.updated_ms), Current::Quake(q)));
    let e = shown_eews(quakes, eews, now_ms)
        .into_iter()
        .next()
        .map(|e| ((e.max_scale, e.received_ms), Current::Eew(e)));
    [q, e].into_iter().flatten().max_by_key(|(k, _)| *k).map(|(_, c)| c)
}

/// 地震の画面に出しているもの (切り出しの判断用)。観測の最大震度だけを数え、緊急地震速報は警報かだけを渡す
pub fn shown(current: Option<Current>) -> Shown {
    match current {
        Some(Current::Quake(q)) => Shown {
            scale: q.max_scale.0,
            warning: false,
        },
        Some(Current::Eew(e)) => Shown {
            scale: -1,
            warning: e.warning,
        },
        None => Shown::NONE,
    }
}

/// 地表での波の半径 (km)。未到達 (発生前・深さより手前) は None。一様な速度の近似 (web の surfaceRadiusKm)
pub fn surface_radius_km(velocity: f64, depth_km: f64, elapsed_sec: f64) -> Option<f64> {
    if elapsed_sec <= 0.0 {
        return None;
    }
    let r = velocity * elapsed_sec;
    let d = depth_km.max(0.0);
    (r > d).then(|| (r * r - d * d).sqrt())
}

/// 描く地震波 1 つ分 (震央と、P 波・S 波の地表での半径)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Wave {
    pub lat: f64,
    pub lon: f64,
    pub p_km: Option<f64>,
    pub s_km: Option<f64>,
}

struct Source {
    lat: f64,
    lon: f64,
    depth_km: f64,
    origin_ms: i64,
}

fn source_of(place: &Place, h: Option<&Hypocenter>, now_ms: u64) -> Option<Source> {
    let (lat, lon, origin_ms) = (place.lat?, place.lon?, place.origin_ms?);
    let depth_km = h.and_then(|h| h.depth_km).map_or(DEFAULT_DEPTH_KM, f64::from);
    (now_ms as i64 - origin_ms <= WAVE_MAX_MS).then_some(Source {
        lat,
        lon,
        depth_km,
        origin_ms,
    })
}

/// 地震波を描く地震。起点は緊急地震速報 (秒単位の発生時刻) を優先し、同じ地震の地震情報 (分単位) では描かない
pub fn waves(quakes: &[QuakeSummary], eews: &[EewSummary], now_ms: u64) -> Vec<Wave> {
    let from_eews: Vec<(Place, Source)> = eews
        .iter()
        .filter_map(|e| {
            let p = eew_place(e);
            Some((p, source_of(&p, e.hypocenter.as_ref(), now_ms)?))
        })
        .collect();
    let from_quakes: Vec<(Place, Source)> = quakes
        .iter()
        .filter_map(|q| {
            let p = quake_place(q);
            Some((p, source_of(&p, q.hypocenter.as_ref(), now_ms)?))
        })
        .filter(|(p, _)| !from_eews.iter().any(|(e, _)| same_quake(e, p)))
        .collect();
    from_eews
        .iter()
        .chain(&from_quakes)
        .map(|(_, s)| {
            let t = (now_ms as i64 - s.origin_ms) as f64 / 1000.0;
            Wave {
                lat: s.lat,
                lon: s.lon,
                p_km: surface_radius_km(VP_KM_S, s.depth_km, t),
                s_km: surface_radius_km(VS_KM_S, s.depth_km, t),
            }
        })
        .collect()
}

/// 震源の印の近くに出す札。地域も県ごとの予測も空で、塗るものが無く、最大予測震度が分かるときだけ
pub fn forecast_tag(e: &EewSummary) -> Option<String> {
    (e.max_scale.is_known() && !e.has_areas && e.pref_scales.is_empty())
        .then(|| format!("予測最大震度{}", e.max_scale.label()))
}

/// 右パネルの見出し。web の詳細の見出しと同じ (「緊急地震速報 (警報) 第3報」)
pub fn eew_kind_text(e: &EewSummary) -> String {
    format!(
        "緊急地震速報 ({}){} 第{}報",
        if e.warning { "警報" } else { "予報" },
        if e.test { " [テスト]" } else { "" },
        e.serial
    )
}

/// 予測の文 (web のバナーと同じ。警報は「強い揺れに警戒」、予報は予測最大震度と県)
pub fn eew_forecast_text(e: &EewSummary) -> String {
    let prefs: Vec<&str> = e.pref_scales.iter().map(|(p, _)| p.as_str()).collect();
    let prefs = if prefs.is_empty() {
        "—".to_string()
    } else {
        prefs.join("・")
    };
    let head = if e.warning {
        "強い揺れに警戒".to_string()
    } else {
        format!("予測最大震度{}", e.max_scale.label())
    };
    format!("{head}: {prefs}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quake::{Eew, EewArea, PrefScale};

    const T0: i64 = 1_790_000_000_000;
    const TOKYO: (f64, f64) = (35.68, 139.76);

    fn hypo(at: (f64, f64), depth: i32) -> Hypocenter {
        Hypocenter {
            name: "x".into(),
            latitude: Some(at.0),
            longitude: Some(at.1),
            depth_km: Some(depth),
            magnitude: Some(5.0),
        }
    }

    fn eew_event(id: &str, serial: u32, recv: u64, f: impl FnOnce(&mut Eew)) -> Event {
        let mut e = Eew {
            event_id: id.into(),
            serial: serial.to_string(),
            cancelled: false,
            test: false,
            warning: true,
            issued_at: "2026/09/30 12:00:05".into(),
            origin_time: Some("2026/09/30 12:00:00".into()),
            origin_time_ms: Some(T0),
            hypocenter: Some(hypo(TOKYO, 10)),
            areas: vec![],
            pref_max: vec![PrefScale {
                pref: "東京都".into(),
                scale: Scale::S5_LOWER,
            }],
            max_scale: Scale::S5_LOWER,
        };
        f(&mut e);
        Event {
            id: format!("{id}-{serial}"),
            source: "test".into(),
            received_at_ms: recv,
            body: EventBody::Eew(e),
        }
    }

    fn quake(updated: u64, origin: i64, scale: Scale, at: Option<(f64, f64)>) -> QuakeSummary {
        QuakeSummary {
            updated_ms: updated,
            origin_time: String::new(),
            origin_ms: Some(origin),
            hypocenter: at.map(|a| hypo(a, 10)),
            max_scale: scale,
            tsunami: "None".into(),
            pref_scales: vec![],
        }
    }

    fn summary(f: impl FnOnce(&mut Eew)) -> EewSummary {
        latest_eews(&[eew_event("a", 1, T0 as u64, f)]).remove(0)
    }

    #[test]
    fn the_latest_serial_wins_per_event_and_a_cancelled_one_is_dropped() {
        let t = T0 as u64;
        let events = [
            eew_event("a", 2, t + 2_000, |e| e.max_scale = Scale::S4),
            eew_event("a", 1, t + 1_000, |e| e.max_scale = Scale::S3), // 届く順が入れ替わっても serial が大きい方
            eew_event("b", 1, t + 1_000, |_| {}),
            eew_event("b", 2, t + 3_000, |e| e.cancelled = true),
        ];
        let l = latest_eews(&events);
        assert_eq!(l.len(), 1);
        assert_eq!((l[0].event_id.as_str(), l[0].serial.as_str()), ("a", "2"));
        assert_eq!((l[0].max_scale, l[0].received_ms), (Scale::S4, t + 2_000));
    }

    #[test]
    fn an_eew_is_shown_for_three_minutes() {
        let t = T0 as u64;
        let l = latest_eews(&[eew_event("a", 1, t, |_| {})]);
        assert!(current(&[], &l, t + 179_999).is_some());
        assert!(current(&[], &l, t + 180_000).is_none());
        assert!(current(&[], &[], t).is_none());
    }

    #[test]
    fn the_quake_report_of_the_same_quake_replaces_the_eew() {
        let t = T0 as u64;
        let l = latest_eews(&[eew_event("a", 1, t, |_| {})]);
        // 発生時刻が 90 秒以内・震央が 200km 以内の地震情報が来たら、そちらを出す
        let q = [quake(t + 40_000, T0 - 5_000, Scale::S3, Some((35.5, 139.7)))];
        assert!(matches!(current(&q, &l, t + 50_000), Some(Current::Quake(_))));
        // 別の地震 (遠い) なら、揺れの大きい緊急地震速報 (予測 5 弱) が出る
        let far = [quake(t + 40_000, T0, Scale::S3, Some((43.0, 141.3)))];
        assert!(matches!(current(&far, &l, t + 50_000), Some(Current::Eew(_))));
        // 震度の大きい地震情報が別にあれば、そちらが先
        let big = [quake(t + 40_000, T0 + 3_600_000, Scale::S6_LOWER, Some((43.0, 141.3)))];
        assert!(matches!(current(&big, &l, t + 50_000), Some(Current::Quake(_))));
    }

    #[test]
    fn the_wave_starts_from_the_eew_and_is_not_drawn_twice() {
        let t = T0 as u64;
        let eews = latest_eews(&[eew_event("a", 1, t + 5_000, |_| {})]);
        // 地震情報の発生時刻は分単位で、EEW と数十秒ずれる。同じ地震なので描くのは EEW の 1 つ
        let same = [quake(t + 60_000, T0 - 20_000, Scale::S4, Some((35.7, 139.8)))];
        let w = waves(&same, &eews, t + 10_000);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].s_km, surface_radius_km(3.75, 10.0, 10.0)); // EEW の秒単位の発生時刻 (T0) から
                                                                    // EEW が無ければ地震情報の発生時刻から
        let only = waves(&same, &[], t + 10_000);
        assert_eq!(only[0].s_km, surface_radius_km(3.75, 10.0, 30.0));
        // 別の地震の地震情報は、EEW の波とは別に描く
        let other = [quake(
            t + 60_000,
            T0 + 3_600_000 - 5_000,
            Scale::S4,
            Some((43.0, 141.3)),
        )];
        assert_eq!(waves(&other, &eews, t + 3_600_000).len(), 1); // 1 時間後: EEW の波は 180 秒で消えている
        let both = waves(&other, &eews, t + 10_000); // 発生前の地震情報は数に入るが、半径はまだ無い
        assert_eq!((both.len(), both[0].s_km.is_some(), both[1].s_km), (2, true, None));
    }

    #[test]
    fn the_wave_radius_follows_depth_and_ends_at_180_seconds() {
        // 一様速度: 震源距離 = v * t、地表の半径 = sqrt(R^2 - depth^2)
        let r = surface_radius_km(VS_KM_S, 30.0, 10.0).unwrap();
        assert!((r - (37.5f64 * 37.5 - 900.0).sqrt()).abs() < 1e-9, "{r}");
        assert_eq!(surface_radius_km(VS_KM_S, 30.0, 8.0), None); // 30km に届く前 (R = 30.0 ちょうど)
        assert_eq!(surface_radius_km(VP_KM_S, 10.0, 0.0), None);
        assert_eq!(surface_radius_km(VP_KM_S, 10.0, -1.0), None);
        assert!(surface_radius_km(VP_KM_S, -5.0, 1.0).is_some()); // 負の深さは 0
        let t = T0 as u64;
        let q = [quake(t, T0, Scale::S4, Some(TOKYO))];
        let w = waves(&q, &[], t + 60_000);
        assert!(w[0].p_km.unwrap() > w[0].s_km.unwrap()); // P 波の方が先へ進む
        assert_eq!(waves(&q, &[], t + 180_000).len(), 1);
        assert_eq!(waves(&q, &[], t + 180_001).len(), 0);
        // 震源や発生時刻が無い地震は描けない
        assert!(waves(&[quake(t, T0, Scale::S4, None)], &[], t).is_empty());
        let mut no_origin = quake(t, T0, Scale::S4, Some(TOKYO));
        no_origin.origin_ms = None;
        assert!(waves(&[no_origin], &[], t).is_empty());
    }

    #[test]
    fn a_depth_is_assumed_when_unknown() {
        let t = T0 as u64;
        let mut q = quake(t, T0, Scale::S4, Some(TOKYO));
        q.hypocenter.as_mut().unwrap().depth_km = None;
        let w = waves(&[q], &[], t + 10_000);
        assert_eq!(w[0].s_km, surface_radius_km(VS_KM_S, 10.0, 10.0));
    }

    #[test]
    fn the_forecast_tag_is_only_for_an_eew_with_nothing_to_paint() {
        fn bare(f: impl FnOnce(&mut Eew)) -> EewSummary {
            summary(|e| {
                e.pref_max.clear();
                f(e)
            })
        }
        assert_eq!(forecast_tag(&bare(|_| {})).as_deref(), Some("予測最大震度5弱"));
        assert_eq!(forecast_tag(&bare(|e| e.max_scale = Scale::UNKNOWN)), None);
        let area = EewArea {
            pref: "東京都".into(),
            name: "東京都２３区".into(),
            scale_from: Scale::S4,
            scale_to: None,
            arrival_time: None,
            arrived: false,
        };
        assert_eq!(forecast_tag(&bare(|e| e.areas = vec![area])), None);
        assert_eq!(forecast_tag(&summary(|_| {})), None); // 県ごとの予測がある
    }

    #[test]
    fn panel_texts_tell_warning_from_forecast() {
        let w = summary(|_| {});
        assert_eq!(eew_kind_text(&w), "緊急地震速報 (警報) 第1報");
        assert_eq!(eew_forecast_text(&w), "強い揺れに警戒: 東京都");
        let f = summary(|e| {
            e.warning = false;
            e.test = true;
            e.pref_max.clear();
        });
        assert_eq!(eew_kind_text(&f), "緊急地震速報 (予報) [テスト] 第1報");
        assert_eq!(eew_forecast_text(&f), "予測最大震度5弱: —");
    }
}
