//! ライブの観測点: 地震情報が届いたらすぐ全部出し、円は描き直さない (2026-11-03 千葉県北東部)

use super::icon::Icons;
use super::model::group_quakes;
use super::step::{frame_waves, Stepper};
use super::zoom_tests::{config_sub, input};
use super::*;
use crate::quake::{Event, EventBody, Hypocenter, ObservationPoint, Quake, QuakeInfoType, Scale, StationPos};

const T0: i64 = 1_790_000_000_000;
/// 日本海の震央 (点が陸の塗りに紛れない)
const EPI: (f64, f64) = (40.5, 134.0);
/// 震央から約 30km 北・約 120km 北の観測点
const NEAR: (f64, f64) = (40.5 + 30.0 / 111.2, 134.0);
const FAR: (f64, f64) = (40.5 + 120.0 / 111.2, 134.0);
/// 観測点の値の入った報の受信時刻 (発生から 3 分: 地震情報の発生時刻からの波は描き終わっている)
const R1: u64 = T0 as u64 + 180_000;

fn station(addr: &str, (lat, lon): (f64, f64), scale: Scale) -> ObservationPoint {
    ObservationPoint {
        pref: "石川県".into(),
        addr: addr.into(),
        is_area: false,
        scale,
        station: Some(StationPos {
            lat,
            lon,
            area: "区域".into(),
        }),
    }
}

/// 最大震度 max の地震の、観測点の値の入った報 (受信は R1)
fn report(max: Scale) -> Vec<Event> {
    vec![Event {
        id: "q1".into(),
        source: "p2pquake".into(),
        received_at_ms: R1,
        body: EventBody::Quake(Quake {
            info_type: QuakeInfoType::DetailScale,
            origin_time: String::new(),
            origin_time_ms: Some(T0),
            issued_at: String::new(),
            hypocenter: Some(Hypocenter {
                name: "日本海".into(),
                latitude: Some(EPI.0),
                longitude: Some(EPI.1),
                depth_km: Some(10),
                magnitude: Some(5.5),
            }),
            max_scale: max,
            domestic_tsunami: "None".into(),
            points: vec![station("近い", NEAR, max), station("遠い", FAR, Scale::S1)],
            pref_max: vec![],
            comment: String::new(),
        }),
    }]
}

#[test]
fn a_late_report_does_not_redraw_the_waves() {
    let events = report(Scale::S3);
    let groups = group_quakes(&events);
    // 発生から 3 分を過ぎているので、受信のあとも波は描かない
    assert!(frame_waves(&groups, &[], true, None, &|_, _| 0.0, R1 + 7_000).is_empty());
    // 波が動かないので、同じ秒の 2 コマ目は描き直さない
    let mut st = Stepper::new(load_renderer(&config_sub(false, false)).unwrap());
    let icons = Icons::new();
    assert!(st.step(&input(&events, R1 + 5_000, &icons)).is_some());
    assert!(st.step(&input(&events, R1 + 5_200, &icons)).is_none());
}

mod drawing {
    use super::super::geo::View;
    use super::super::model::scale_color;
    use super::super::shaken::Point;
    use super::super::tests::{map_rect, quake, renderer, rgb, scene};
    use super::super::text::Text;
    use super::*;

    /// 震央から約 60km 北・約 200km 北 (震央の印と重ならない)
    const NEAR60: (f64, f64) = (40.5 + 60.0 / 111.2, 134.0);
    const FAR200: (f64, f64) = (40.5 + 200.0 / 111.2, 134.0);

    fn dot(addr: &str, (lat, lon): (f64, f64), scale: Scale) -> Point {
        Point {
            addr: addr.into(),
            is_area: false,
            scale,
            station: Some((lat, lon, "区域".into())),
        }
    }

    fn spot((lat, lon): (f64, f64)) -> (u32, u32) {
        let (x, y) = View::fit_home(map_rect()).px(lon, lat);
        (x as u32, y as u32)
    }

    #[test]
    fn every_dot_is_drawn_as_soon_as_the_report_arrives() {
        let mut r = renderer(Text::none());
        let mut g = quake(Scale::S4, &[("石川県", Scale::S4)], Some(EPI));
        g.points = vec![dot("近い", NEAR60, Scale::S4), dot("遠い", FAR200, Scale::S3)];
        let pm = r.render(&scene(Some(&g), &[], None, None));
        assert_eq!(rgb(&pm, spot(NEAR60)), scale_color(Scale::S4));
        assert_eq!(rgb(&pm, spot(FAR200)), scale_color(Scale::S3));
    }
}

#[test]
fn an_area_only_follow_up_keeps_the_station_dots() {
    let mut events = report(Scale::S3);
    let mut area_only = events[0].clone();
    area_only.id = "q2".into();
    area_only.received_at_ms = R1 + 10_000;
    let EventBody::Quake(q) = &mut area_only.body else {
        panic!()
    };
    q.info_type = QuakeInfoType::ScalePrompt;
    q.points = vec![ObservationPoint {
        is_area: true,
        station: None,
        ..q.points[0].clone()
    }];
    events.push(area_only);
    let groups = group_quakes(&events);
    // 区域だけの続報が来ても、観測点を含む報の点を使う (点が消えない)
    assert!(groups[0].points.iter().any(|p| !p.is_area));
}
