//! 揺れの広がりの再現 (docs/superpowers/specs/2026-10-18-quake-spread-replay-design.md): コマへの組み込みと描画

use super::icon::Icons;
use super::model::group_quakes;
use super::spread::{self, spread_of, Spread, LEAD_MS};
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
pub(super) const R1: u64 = T0 as u64 + 180_000;

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
pub(super) fn report(max: Scale) -> Vec<Event> {
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

fn spread_now(events: &[Event], now: u64) -> Spread {
    let r = load_renderer(&config_sub(false, false)).unwrap();
    let groups = group_quakes(events);
    spread_of(events, &groups[0], r.zones(), now).unwrap()
}

#[test]
fn the_spread_wave_replaces_the_quake_wave() {
    let events = report(Scale::S3);
    let groups = group_quakes(&events);
    let now = R1 + LEAD_MS + 5_000;
    // 発生から 3 分を過ぎているので、地震情報の発生時刻からの波は無い
    assert!(frame_waves(&groups, &[], true, None, None, &|_, _| 0.0, now).is_empty());
    let s = spread_now(&events, now);
    let waves = frame_waves(&groups, &[], true, None, Some(&s), &|_, _| 0.0, now);
    assert_eq!(waves, vec![spread::wave(&s, now)]);
}

#[test]
fn the_frame_is_redrawn_every_tick_while_spreading() {
    let mut st = Stepper::new(load_renderer(&config_sub(false, false)).unwrap());
    let icons = Icons::new();
    let start = R1 + LEAD_MS;
    // 波が動いている間は、同じ秒の中でもコマごとに描き直す (再現が無ければ同じ秒の 2 コマ目は None)
    for (events, want) in [(report(Scale::S3), true), (report(Scale::S2), false)] {
        assert!(st.step(&input(&events, start + 3_000, &icons)).is_some());
        assert_eq!(st.step(&input(&events, start + 3_200, &icons)).is_some(), want);
    }
}

mod drawing {
    use super::super::geo::View;
    use super::super::model::scale_color;
    use super::super::paint::SEA;
    use super::super::shaken::Point;
    use super::super::tests::{map_rect, quake, renderer, rgb, scene};
    use super::super::text::Text;
    use super::*;

    /// 震央から約 60km 北・約 200km 北 (震央の印と重ならない)
    const NEAR60: (f64, f64) = (40.5 + 60.0 / 111.2, 134.0);
    const FAR200: (f64, f64) = (40.5 + 200.0 / 111.2, 134.0);
    const START: u64 = 1_800_000_000_000;

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

    fn spreading() -> Spread {
        Spread {
            lat: EPI.0,
            lon: EPI.1,
            depth_km: 10.0,
            start_ms: START,
            end_ms: START + 60_000,
        }
    }

    #[test]
    fn only_the_reached_dots_are_drawn_while_spreading() {
        let mut r = renderer(Text::none());
        let mut g = quake(Scale::S4, &[("石川県", Scale::S4)], Some(EPI));
        g.points = vec![dot("近い", NEAR60, Scale::S4), dot("遠い", FAR200, Scale::S3)];
        // 20 秒後: S 波は地表で約 74km。近い点だけが灯る
        let mut sc = scene(Some(&g), &[], None, None);
        sc.now_ms = START + 20_000;
        sc.spread = Some(spreading());
        let pm = r.render(&sc);
        assert_eq!(rgb(&pm, spot(NEAR60)), scale_color(Scale::S4));
        assert_eq!(rgb(&pm, spot(FAR200)), SEA);
        // 再現が無ければ、どちらも灯る (今のまま)
        sc.spread = None;
        let pm = r.render(&sc);
        assert_eq!(rgb(&pm, spot(NEAR60)), scale_color(Scale::S4));
        assert_eq!(rgb(&pm, spot(FAR200)), scale_color(Scale::S3));
    }

    #[test]
    fn the_spread_label_is_drawn_only_while_spreading() {
        let font = BroadcastConfig::default().font;
        let mut r = renderer(Text::load(&font, 0).unwrap_or_else(|_| Text::none()));
        if !r.text.enabled() {
            return;
        }
        let g = quake(Scale::S4, &[("石川県", Scale::S4)], Some(EPI));
        let mut sc = scene(Some(&g), &[], None, None);
        sc.now_ms = START + 20_000;
        let without = r.render(&sc);
        sc.spread = Some(spreading());
        let with = r.render(&sc);
        // 本図の右上 (左上は南西諸島の別枠と重なる)
        let (x, y, w, _) = map_rect();
        let (right, y) = ((x + w) as u32, y as u32);
        let in_corner = |pm: &tiny_skia::Pixmap, from: u32, to: u32| {
            (y + 6..y + 30)
                .flat_map(|yy| (from..to).map(move |xx| (xx, yy)))
                .any(|p| rgb(pm, p) != rgb(&without, p))
        };
        assert!(in_corner(&with, right - 200, right - 8));
        assert!(!in_corner(&with, x as u32 + 8, x as u32 + 240));
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
    // 区域だけの続報が来ても、観測点を含む報の点を使う (再現の最中に点が消えない)
    assert!(groups[0].points.iter().any(|p| !p.is_area));
}
