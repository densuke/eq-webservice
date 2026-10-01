//! 記録から描き直すとき、のちの報で分かる震源を最初から薄く出すための判断 (純粋な関数)。
//! web/src/history.ts の hindsightOf、quakes.ts の pendingHindsight・waveSources と同じ規則 (docs/quake-archive.md 2.6 章)。
//! ライブの配信は使わない (渡さない)。

use crate::quake::{Event, EventBody, Hypocenter, QuakeInfoType};

use super::eew::{
    eew_place, quake_place, surface_radius_km, EewSummary, Wave, DEFAULT_DEPTH_KM, VP_KM_S, VS_KM_S, WAVE_MAX_MS,
};
use super::model::{same_quake, Place, QuakeSummary};

/// のちの報で分かった震源と発生時刻
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hindsight {
    pub lat: f64,
    pub lon: f64,
    pub depth_km: f64,
    /// 発生時刻 (epoch ミリ秒)。緊急地震速報の秒単位を優先し、無ければ地震情報の分単位 (起点が最大 59 秒ずれる)
    pub origin_ms: i64,
}

fn center(h: Option<&Hypocenter>) -> Option<(f64, f64, f64)> {
    let h = h?;
    Some((
        h.latitude?,
        h.longitude?,
        h.depth_km.map_or(DEFAULT_DEPTH_KM, f64::from),
    ))
}

/// 震源の情報のうち、後ろの報ほど確からしいもの: 各地の震度 → 震源の情報 (震度速報の震源は無い)
fn rank(t: QuakeInfoType) -> Option<u8> {
    match t {
        QuakeInfoType::DetailScale => Some(0),
        QuakeInfoType::Destination | QuakeInfoType::ScaleAndDestination => Some(1),
        _ => None,
    }
}

/// 集めた報 (同じ地震だけ) から、のちに分かる震源を探す。優先順位は 各地の震度 → 震源の情報 → 緊急地震速報の最終報。
/// 見つからなければ None
pub fn hindsight_of(events: &[Event]) -> Option<Hindsight> {
    let quakes: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.body {
            EventBody::Quake(q) => Some((e, q, rank(q.info_type)?, center(q.hypocenter.as_ref())?)),
            _ => None,
        })
        .collect();
    let best = quakes.iter().map(|x| x.2).min();
    // 同じ時刻なら後ろの報 (max_by_key は最後の最大を返す)
    let from_quake = quakes
        .iter()
        .filter(|x| Some(x.2) == best)
        .max_by_key(|x| x.0.received_at_ms)
        .map(|x| x.3);
    let from_eew = || {
        events
            .iter()
            .filter_map(|e| match &e.body {
                EventBody::Eew(x) if !x.cancelled => Some((e.received_at_ms, center(x.hypocenter.as_ref())?)),
                _ => None,
            })
            .max_by_key(|x| x.0)
            .map(|x| x.1)
    };
    let (lat, lon, depth_km) = from_quake.or_else(from_eew)?;
    let eew_origin = events
        .iter()
        .filter_map(|e| match &e.body {
            EventBody::Eew(x) => x.origin_time_ms,
            _ => None,
        })
        .min();
    let quake_origin = || {
        events
            .iter()
            .filter_map(|e| match &e.body {
                EventBody::Quake(q) => Some((e.received_at_ms, q.origin_time_ms?)),
                _ => None,
            })
            .max_by_key(|x| x.0)
            .map(|x| x.1)
    };
    Some(Hindsight {
        lat,
        lon,
        depth_km,
        origin_ms: eew_origin.or_else(quake_origin)?,
    })
}

/// まだ本物の震源 (同じ地震の報で震源の付いたもの) が届いていないときだけ、のちに分かった震源を返す
pub fn pending<'a>(h: Option<&'a Hindsight>, quakes: &[QuakeSummary], eews: &[EewSummary]) -> Option<&'a Hindsight> {
    let h = h?;
    let me = Place {
        origin_ms: Some(h.origin_ms),
        lat: Some(h.lat),
        lon: Some(h.lon),
    };
    let located = |p: Place, has_center: bool| has_center && same_quake(&p, &me);
    let found = quakes
        .iter()
        .any(|q| located(quake_place(q), center(q.hypocenter.as_ref()).is_some()))
        || eews
            .iter()
            .any(|e| located(eew_place(e), center(e.hypocenter.as_ref()).is_some()));
    (!found).then_some(h)
}

/// 描く波。発生前と、発生から WAVE_MAX_MS を過ぎたあとは None
pub fn wave(h: &Hindsight, now_ms: u64) -> Option<Wave> {
    let elapsed = now_ms as i64 - h.origin_ms;
    if !(0..=WAVE_MAX_MS).contains(&elapsed) {
        return None;
    }
    let t = elapsed as f64 / 1000.0;
    Some(Wave {
        lat: h.lat,
        lon: h.lon,
        p_km: surface_radius_km(VP_KM_S, h.depth_km, t),
        s_km: surface_radius_km(VS_KM_S, h.depth_km, t),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quake::{Eew, Quake, Scale};

    const T0: i64 = 1_790_000_000_000;

    fn hypo(lat: f64, depth: Option<i32>) -> Hypocenter {
        Hypocenter {
            name: "x".into(),
            latitude: Some(lat),
            longitude: Some(139.0),
            depth_km: depth,
            magnitude: Some(5.0),
        }
    }

    fn quake(recv: u64, info: QuakeInfoType, lat: Option<f64>, origin: i64) -> Event {
        Event {
            id: format!("q{recv}"),
            source: "p2pquake".into(),
            received_at_ms: recv,
            body: EventBody::Quake(Quake {
                info_type: info,
                origin_time: String::new(),
                origin_time_ms: Some(origin),
                issued_at: String::new(),
                hypocenter: lat.map(|l| hypo(l, Some(20))),
                max_scale: Scale::S4,
                domestic_tsunami: String::new(),
                points: vec![],
                pref_max: vec![],
                comment: String::new(),
            }),
        }
    }

    fn eew(recv: u64, lat: Option<f64>, origin: i64, cancelled: bool) -> Event {
        Event {
            id: format!("e{recv}"),
            source: "wolfx".into(),
            received_at_ms: recv,
            body: EventBody::Eew(Eew {
                event_id: "E".into(),
                serial: "1".into(),
                cancelled,
                test: false,
                warning: false,
                issued_at: String::new(),
                origin_time: None,
                origin_time_ms: Some(origin),
                hypocenter: lat.map(|l| hypo(l, None)),
                areas: vec![],
                pref_max: vec![],
                max_scale: Scale::S3,
            }),
        }
    }

    #[test]
    fn detail_scale_beats_destination_beats_the_last_eew() {
        let t = T0 as u64;
        let events = [
            eew(t, Some(30.0), T0, false),
            quake(t + 1, QuakeInfoType::Destination, Some(31.0), T0 - 20_000),
            quake(t + 2, QuakeInfoType::DetailScale, Some(32.0), T0 - 20_000),
            quake(t + 3, QuakeInfoType::ScalePrompt, Some(33.0), T0 - 20_000),
        ];
        assert_eq!(hindsight_of(&events).unwrap().lat, 32.0);
        assert_eq!(hindsight_of(&events[..2]).unwrap().lat, 31.0);
        // 震度速報には震源が無い扱い。無ければ緊急地震速報の最終報
        assert_eq!(hindsight_of(&[events[0].clone(), events[3].clone()]).unwrap().lat, 30.0);
        assert_eq!(hindsight_of(&[events[3].clone()]), None);
    }

    #[test]
    fn the_origin_is_the_eew_second_if_any_else_the_quake_minute() {
        let t = T0 as u64;
        let q = quake(t, QuakeInfoType::DetailScale, Some(32.0), T0 - 50_000);
        let e = eew(t, None, T0 - 3_000, false);
        let h = hindsight_of(&[q.clone(), e]).unwrap();
        assert_eq!((h.origin_ms, h.depth_km), (T0 - 3_000, 20.0));
        assert_eq!(hindsight_of(&[q]).unwrap().origin_ms, T0 - 50_000);
    }

    #[test]
    fn a_cancelled_eew_gives_no_center_and_a_missing_depth_is_ten_km() {
        let t = T0 as u64;
        assert_eq!(hindsight_of(&[eew(t, Some(30.0), T0, true)]), None);
        assert_eq!(hindsight_of(&[eew(t, Some(30.0), T0, false)]).unwrap().depth_km, 10.0);
    }

    #[test]
    fn it_is_pending_until_a_located_report_of_the_same_quake_arrives() {
        let t = T0 as u64;
        let h = hindsight_of(&[quake(t + 90_000, QuakeInfoType::DetailScale, Some(32.0), T0)]).unwrap();
        let unlocated =
            crate::broadcast::native::model::group_quakes(&[quake(t, QuakeInfoType::ScalePrompt, None, T0)]);
        assert!(pending(Some(&h), &unlocated, &[]).is_some());
        let located =
            crate::broadcast::native::model::group_quakes(&[quake(t, QuakeInfoType::Destination, Some(32.0), T0)]);
        assert!(pending(Some(&h), &located, &[]).is_none());
        assert!(pending(None, &located, &[]).is_none());
    }

    #[test]
    fn the_wave_grows_from_the_origin_and_ends_after_180_seconds() {
        let h = Hindsight {
            lat: 35.0,
            lon: 139.0,
            depth_km: 10.0,
            origin_ms: T0,
        };
        assert_eq!(wave(&h, T0 as u64 - 1), None);
        let w = wave(&h, T0 as u64 + 10_000).unwrap();
        assert!((w.p_km.unwrap() - (65.0f64 * 65.0 - 100.0).sqrt()).abs() < 1e-6);
        assert!(wave(&h, T0 as u64 + 180_000).is_some());
        assert_eq!(wave(&h, T0 as u64 + 180_001), None);
    }
}
