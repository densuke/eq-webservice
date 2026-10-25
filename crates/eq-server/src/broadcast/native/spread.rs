//! 揺れの広がりの再現 (docs/superpowers/specs/2026-10-18-quake-spread-replay-design.md) の判断 (純粋な関数)。
//! 観測点の値が入った地震情報が届いたら、発生時刻から波を描き直し、S 波が届いた観測点から灯す。
//! 状態は持たない: 届いた報と今の時刻だけで決める (ライブも記録からの描き直しも同じに動く)。

use super::eew::{quake_place, surface_radius_km, Wave, DEFAULT_DEPTH_KM, VP_KM_S, VS_KM_S, WAVE_LIMIT_MS};
use super::model::{distance_km, event_place, same_quake, QuakeSummary, SAME_QUAKE_KM};
use super::shaken::Zones;
use crate::quake::{Event, EventBody, ObservationPoint, Scale};

/// 流す報が届いてから、再現の発生時刻 0 秒までの間 (点を隠して案内だけを出す)
pub const LEAD_MS: u64 = 2_000;
/// 一番遠い点に S 波が届いてから終わるまで
pub const HOLD_MS: u64 = 5_000;
/// 流す最大震度の下限
const MIN_SCALE: Scale = Scale::S3;

/// いま流している再現。start_ms は再現の発生時刻 0 秒 (流す報の受信時刻 + LEAD_MS)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spread {
    pub lat: f64,
    pub lon: f64,
    pub depth_km: f64,
    pub start_ms: u64,
    pub end_ms: u64,
}

/// 地震の画面の地震 current について、いま再現の最中なら返す。
/// 流す報は、同じ地震の報のうち観測点 (区域ではない点) を初めて含むもの。その時点の最大震度が 3 以上のときだけ。
/// その報より後に緊急地震速報か別の地震の地震情報が届いていたら流さない
pub fn spread_of(events: &[Event], current: &QuakeSummary, zones: &Zones, now_ms: u64) -> Option<Spread> {
    let place = quake_place(current);
    let (lat, lon) = (place.lat?, place.lon?);
    let mine =
        |e: &&Event| matches!(e.body, EventBody::Quake(_)) && event_place(e).is_some_and(|p| same_quake(&p, &place));
    // 流す報: 同じ地震で観測点を含む、最も早く届いた報 (同時刻なら届いた順で先のもの)。配列を作らずに求める
    let trigger = events
        .iter()
        .filter(mine)
        .filter(|e| stations(e).is_some())
        .min_by_key(|e| e.received_at_ms)?;
    let received = trigger.received_at_ms;
    // 再現の窓 (受信から LEAD_MS + 上限) の外は、ほかの計算をせずに返す (地震の画面のあいだ毎コマ呼ばれる)
    if now_ms < received || now_ms > received + LEAD_MS + WAVE_LIMIT_MS as u64 {
        return None;
    }
    let points = stations(trigger)?;
    let max = events
        .iter()
        .filter(mine)
        .filter(|e| e.received_at_ms <= received)
        .filter_map(|e| match &e.body {
            EventBody::Quake(q) => Some(q.max_scale),
            _ => None,
        })
        .max()?;
    if max < MIN_SCALE {
        return None;
    }
    let interrupted = events.iter().any(|e| {
        e.received_at_ms > received
            && e.received_at_ms <= now_ms
            && match &e.body {
                EventBody::Eew(_) => true,
                // 発生時刻の分からない報は、別の地震か判断できないので止めない
                EventBody::Quake(_) => event_place(e).is_some_and(|p| p.origin_ms.is_some() && !same_quake(&p, &place)),
                _ => false,
            }
    });
    if interrupted {
        return None;
    }
    let depth_km = current
        .hypocenter
        .as_ref()
        .and_then(|h| h.depth_km)
        .map_or(DEFAULT_DEPTH_KM, f64::from)
        .max(0.0);
    // 終わりは流す報の観測点で決める。位置は記録の値を優先し、無ければ観測点の表 (Zones::dots と同じ)
    let far = points
        .iter()
        .filter(|p| !p.is_area)
        .filter_map(|p| {
            p.station
                .as_ref()
                .map(|s| (s.lat, s.lon))
                .or_else(|| zones.position(&p.addr))
        })
        .map(|(dlat, dlon)| distance_km(lat, lon, dlat, dlon))
        .reduce(f64::max)?;
    let start_ms = received + LEAD_MS;
    let arrive_ms = ((far * far + depth_km * depth_km).sqrt() / VS_KM_S * 1000.0).ceil() as u64;
    let end_ms = start_ms + (arrive_ms + HOLD_MS).min(WAVE_LIMIT_MS as u64);
    (now_ms <= end_ms).then_some(Spread {
        lat,
        lon,
        depth_km,
        start_ms,
        end_ms,
    })
}

/// 観測点 (区域ではない点) を含む地震情報の点
fn stations(e: &Event) -> Option<&[ObservationPoint]> {
    match &e.body {
        EventBody::Quake(q) if q.points.iter().any(|p| !p.is_area) => Some(&q.points),
        _ => None,
    }
}

fn elapsed_sec(s: &Spread, now_ms: u64) -> f64 {
    (now_ms as f64 - s.start_ms as f64) / 1000.0
}

/// S 波の地表での半径 (km)。開始前・まだ地表に届かないときは None (点を全部隠す)
pub fn reach_km(s: &Spread, now_ms: u64) -> Option<f64> {
    surface_radius_km(VS_KM_S, s.depth_km, elapsed_sec(s, now_ms))
}

/// 再現の円 (P 波・S 波)
pub fn wave(s: &Spread, now_ms: u64) -> Wave {
    let t = elapsed_sec(s, now_ms);
    Wave {
        lat: s.lat,
        lon: s.lon,
        p_km: surface_radius_km(VP_KM_S, s.depth_km, t),
        s_km: surface_radius_km(VS_KM_S, s.depth_km, t),
    }
}

/// 再現と同じ地震の波か (震央が近い)。再現の最中は、その地震の今の波を描かない
pub fn near(s: &Spread, w: &Wave) -> bool {
    distance_km(s.lat, s.lon, w.lat, w.lon) <= SAME_QUAKE_KM
}

/// その点 (経度, 緯度) に S 波が届いたか
pub fn lit(s: &Spread, now_ms: u64, lon: f64, lat: f64) -> bool {
    reach_km(s, now_ms).is_some_and(|r| distance_km(s.lat, s.lon, lat, lon) <= r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broadcast::native::model::{distance_km, group_quakes};
    use crate::broadcast::native::shaken::{Stations, Zones};
    use crate::quake::{Eew, Event, EventBody, Hypocenter, ObservationPoint, Quake, QuakeInfoType, Scale};

    const T0: i64 = 1_790_000_000_000;
    const LAT: f64 = 35.0;
    const LON: f64 = 139.0;
    /// 震央から約 30km 北・約 120km 北の観測点
    const NEAR: (f64, f64) = (35.0 + 30.0 / 111.2, 139.0);
    const FAR: (f64, f64) = (35.0 + 120.0 / 111.2, 139.0);
    const R0: u64 = T0 as u64 + 90_000;
    const R1: u64 = T0 as u64 + 180_000;

    fn zones(extra: &[(&str, (f64, f64))]) -> Zones {
        let st: Stations = [("近い", NEAR), ("遠い", FAR)]
            .iter()
            .chain(extra)
            .map(|(n, (lat, lon))| (n.to_string(), (*lat, *lon, "区域".to_string())))
            .collect();
        Zones::new(vec![], &[], st)
    }

    fn point(addr: &str, is_area: bool, scale: Scale) -> ObservationPoint {
        ObservationPoint {
            pref: "東京都".into(),
            addr: addr.into(),
            is_area,
            scale,
            station: None,
        }
    }

    fn quake(recv: u64, at: Option<(f64, f64)>, max: Scale, points: Vec<ObservationPoint>) -> Event {
        Event {
            id: format!("q{recv}"),
            source: "p2pquake".into(),
            received_at_ms: recv,
            body: EventBody::Quake(Quake {
                info_type: if points.iter().any(|p| !p.is_area) {
                    QuakeInfoType::DetailScale
                } else {
                    QuakeInfoType::ScalePrompt
                },
                origin_time: String::new(),
                origin_time_ms: Some(T0),
                issued_at: String::new(),
                hypocenter: at.map(|(lat, lon)| Hypocenter {
                    name: "x".into(),
                    latitude: Some(lat),
                    longitude: Some(lon),
                    depth_km: Some(10),
                    magnitude: Some(5.0),
                }),
                max_scale: max,
                domestic_tsunami: String::new(),
                points,
                pref_max: vec![],
                comment: String::new(),
            }),
        }
    }

    fn eew(recv: u64) -> Event {
        Event {
            id: format!("e{recv}"),
            source: "wolfx".into(),
            received_at_ms: recv,
            body: EventBody::Eew(Eew {
                event_id: "E".into(),
                serial: "1".into(),
                cancelled: false,
                test: false,
                warning: false,
                issued_at: String::new(),
                origin_time: None,
                origin_time_ms: Some(T0),
                hypocenter: None,
                areas: vec![],
                pref_max: vec![],
                max_scale: Scale::S3,
            }),
        }
    }

    fn stations_report(recv: u64, max: Scale) -> Event {
        quake(
            recv,
            Some((LAT, LON)),
            max,
            vec![point("近い", false, max), point("遠い", false, Scale::S1)],
        )
    }

    fn base(max: Scale) -> Vec<Event> {
        vec![
            quake(R0, Some((LAT, LON)), max, vec![point("東京都２３区", true, max)]),
            stations_report(R1, max),
        ]
    }

    fn spread_at(events: &[Event], z: &Zones, now: u64) -> Option<Spread> {
        let groups = group_quakes(events);
        let mine = groups
            .iter()
            .find(|g| g.hypocenter.as_ref().and_then(|h| h.latitude) == Some(LAT))
            .unwrap_or(&groups[0]);
        spread_of(events, mine, z, now)
    }

    /// 震央からの距離 d (km) の点に S 波が届くまでの ms (深さ 10km)
    fn arrive_ms(d: f64) -> u64 {
        ((d * d + 100.0).sqrt() / VS_KM_S * 1000.0).ceil() as u64
    }

    #[test]
    fn spreads_from_the_first_report_with_stations() {
        let (ev, z) = (base(Scale::S3), zones(&[]));
        assert_eq!(spread_at(&ev, &z, R1 - 1), None);
        let s = spread_at(&ev, &z, R1).unwrap();
        assert_eq!(s.start_ms, R1 + LEAD_MS);
        assert_eq!((s.lat, s.lon, s.depth_km), (LAT, LON, 10.0));
    }

    #[test]
    fn scale_two_does_not_spread() {
        assert_eq!(spread_at(&base(Scale::S2), &zones(&[]), R1 + 5_000), None);
    }

    #[test]
    fn no_epicenter_no_spread() {
        let ev = vec![quake(
            R1,
            None,
            Scale::S3,
            vec![point("近い", false, Scale::S3), point("遠い", false, Scale::S1)],
        )];
        assert_eq!(spread_at(&ev, &zones(&[]), R1 + 5_000), None);
    }

    #[test]
    fn no_located_dots_no_spread() {
        let ev = vec![quake(
            R1,
            Some((LAT, LON)),
            Scale::S3,
            vec![point("表に無い観測点", false, Scale::S3)],
        )];
        assert_eq!(spread_at(&ev, &zones(&[]), R1 + 5_000), None);
    }

    #[test]
    fn dots_light_only_inside_the_s_wave() {
        let (ev, z) = (base(Scale::S3), zones(&[]));
        let s = spread_at(&ev, &z, R1).unwrap();
        assert_eq!(reach_km(&s, s.start_ms - 1), None);
        assert!(!lit(&s, s.start_ms, NEAR.1, NEAR.0));
        assert!(!lit(&s, s.start_ms, FAR.1, FAR.0));
        let t = s.start_ms + 9_000;
        assert!(lit(&s, t, NEAR.1, NEAR.0));
        assert!(!lit(&s, t, FAR.1, FAR.0));
        let w = wave(&s, t);
        assert_eq!((w.lat, w.lon), (LAT, LON));
        assert!(w.p_km.unwrap() > w.s_km.unwrap());
        assert_eq!(w.s_km, reach_km(&s, t));
    }

    #[test]
    fn ends_five_seconds_after_the_farthest_dot() {
        let (ev, z) = (base(Scale::S3), zones(&[]));
        let s = spread_at(&ev, &z, R1).unwrap();
        let far = distance_km(LAT, LON, FAR.0, FAR.1);
        assert_eq!(s.end_ms, s.start_ms + arrive_ms(far) + HOLD_MS);
        assert!(spread_at(&ev, &z, s.end_ms).is_some());
        assert_eq!(spread_at(&ev, &z, s.end_ms + 1), None);
        // とても遠い点があっても、波を描く上限を超えない
        let z = zones(&[("とても遠い", (35.0 + 2000.0 / 111.2, 139.0))]);
        let mut ev = base(Scale::S3);
        let EventBody::Quake(q) = &mut ev[1].body else { panic!() };
        q.points.push(point("とても遠い", false, Scale::S1));
        let s = spread_at(&ev, &z, R1).unwrap();
        assert_eq!(s.end_ms, s.start_ms + WAVE_LIMIT_MS as u64);
    }

    #[test]
    fn a_later_eew_or_another_quake_stops_it() {
        let z = zones(&[]);
        let mut ev = base(Scale::S3);
        ev.push(eew(R1 + 10_000));
        assert!(spread_at(&ev, &z, R1 + 9_999).is_some());
        assert_eq!(spread_at(&ev, &z, R1 + 10_000), None);
        let mut ev = base(Scale::S3);
        // 震央が 500km 離れた別の地震
        ev.push(quake(R1 + 10_000, Some((LAT + 500.0 / 111.2, LON)), Scale::S1, vec![]));
        assert!(spread_at(&ev, &z, R1 + 9_999).is_some());
        assert_eq!(spread_at(&ev, &z, R1 + 10_000), None);
    }

    #[test]
    fn an_eew_before_the_report_does_not_stop_it() {
        let mut ev = vec![eew(R0 - 60_000)];
        ev.extend(base(Scale::S3));
        assert!(spread_at(&ev, &zones(&[]), R1 + 5_000).is_some());
    }

    #[test]
    fn follow_up_reports_neither_restart_nor_stop() {
        let z = zones(&[]);
        let mut ev = base(Scale::S3);
        ev.push(stations_report(R1 + 10_000, Scale::S3));
        ev.push(quake(
            R1 + 11_000,
            Some((LAT, LON)),
            Scale::S3,
            vec![point("東京都２３区", true, Scale::S3)],
        ));
        let s = spread_at(&ev, &z, R1 + 12_000).unwrap();
        assert_eq!(s.start_ms, R1 + LEAD_MS);
    }

    #[test]
    fn the_end_is_measured_on_the_report_that_started_it() {
        let z = zones(&[]);
        let first = spread_at(&base(Scale::S3), &z, R1).unwrap();
        // 後の訂正で遠い点が無くなっても、終わりは流す報の点で決まる
        let mut ev = base(Scale::S3);
        ev.push(quake(
            R1 + 10_000,
            Some((LAT, LON)),
            Scale::S3,
            vec![point("近い", false, Scale::S3)],
        ));
        let s = spread_at(&ev, &z, R1 + 12_000).unwrap();
        assert_eq!(s.end_ms, first.end_ms);
    }

    #[test]
    fn a_report_without_an_origin_time_does_not_stop_it() {
        let mut ev = base(Scale::S3);
        let mut odd = quake(R1 + 10_000, Some((LAT + 500.0 / 111.2, LON)), Scale::S1, vec![]);
        let EventBody::Quake(q) = &mut odd.body else { panic!() };
        q.origin_time_ms = None;
        ev.push(odd);
        // 発生時刻が分からない報は、別の地震かどうか判断できないので止めない
        assert!(spread_at(&ev, &zones(&[]), R1 + 12_000).is_some());
    }

    #[test]
    fn the_window_ends_at_lead_plus_the_wave_limit() {
        // とても遠い点で、終わりが上限 (受信 + LEAD_MS + WAVE_LIMIT_MS) にかかる
        let z = zones(&[("とても遠い", (35.0 + 2000.0 / 111.2, 139.0))]);
        let mut ev = base(Scale::S3);
        let EventBody::Quake(q) = &mut ev[1].body else { panic!() };
        q.points.push(point("とても遠い", false, Scale::S1));
        let edge = R1 + LEAD_MS + WAVE_LIMIT_MS as u64;
        assert!(spread_at(&ev, &z, edge - 1).is_some());
        assert_eq!(spread_at(&ev, &z, edge).map(|s| s.end_ms), Some(edge));
        assert_eq!(spread_at(&ev, &z, edge + 1), None);
    }
}
