//! 表示の判断 (純粋な関数)。時刻は引数で受け取り、時計や通信には触れない。
//! web/src/priority.ts (sameQuake・settleMs・byPriority)、groups.ts (summarizeQuake)、scale.ts の色と同じ結果にする。

use std::collections::BTreeMap;

use super::shaken::Point;
use crate::broadcast::mixer::AlertLevel;
use crate::quake::model::{Tsunami, TsunamiGrade};
use crate::quake::{jst, Event, EventBody, Hypocenter, Quake, Scale};

/// 同じ地震とみなす発生時刻の差と震央の距離
const SAME_QUAKE_MS: i64 = 90_000;
const SAME_QUAKE_KM: f64 = 200.0;
/// 最後の情報から地震の画面を出し続ける時間 (震度 2 以下は短い)
const SETTLE_MS: u64 = 3 * 60_000;
const MINOR_SETTLE_MS: u64 = 60_000;
const MINOR_SCALE: i32 = 20;
/// BGM の音量 (画面の既定と同じ)
const BGM_VOLUME: f32 = 0.4;

pub fn settle_ms(scale: Scale) -> u64 {
    if scale.0 > 0 && scale.0 <= MINOR_SCALE {
        MINOR_SETTLE_MS
    } else {
        SETTLE_MS
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    pub origin_ms: Option<i64>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
}

/// 2 点の距離 (km。球面)
pub fn distance_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let a = ((lat2 - lat1).to_radians() / 2.0).sin().powi(2)
        + p1.cos() * p2.cos() * ((lon2 - lon1).to_radians() / 2.0).sin().powi(2);
    2.0 * 6371.0 * a.sqrt().min(1.0).asin()
}

pub fn same_quake(a: &Place, b: &Place) -> bool {
    let (Some(x), Some(y)) = (a.origin_ms, b.origin_ms) else {
        return false;
    };
    if (x - y).abs() > SAME_QUAKE_MS {
        return false;
    }
    match (a.lat, a.lon, b.lat, b.lon) {
        (Some(la), Some(oa), Some(lb), Some(ob)) => distance_km(la, oa, lb, ob) <= SAME_QUAKE_KM,
        _ => true,
    }
}

/// 同じ地震の情報をまとめたもの
#[derive(Debug, Clone, PartialEq)]
pub struct QuakeSummary {
    /// 最後の情報を受けた時刻 (サーバの時計。epoch ミリ秒)
    pub updated_ms: u64,
    pub origin_time: String,
    /// 発生時刻 (epoch ミリ秒。地震波の起点)
    pub origin_ms: Option<i64>,
    pub hypocenter: Option<Hypocenter>,
    pub max_scale: Scale,
    pub tsunami: String,
    /// 都道府県ごとの最大震度
    pub pref_scales: Vec<(String, Scale)>,
    /// 観測点 (寄りの範囲に使う。観測点のある最新の報のもの)
    pub points: Vec<Point>,
}

fn place_of(q: &Quake) -> Place {
    let h = q.hypocenter.as_ref();
    Place {
        origin_ms: q.origin_time_ms.or_else(|| jst::parse_ms(&q.origin_time)),
        lat: h.and_then(|h| h.latitude),
        lon: h.and_then(|h| h.longitude),
    }
}

/// 地震情報・緊急地震速報 1 件の場所 (ほかの種類は None)。動画に入れる報を、同じ地震に絞るのに使う
pub fn event_place(e: &Event) -> Option<Place> {
    match &e.body {
        EventBody::Quake(q) => Some(place_of(q)),
        EventBody::Eew(x) => {
            let h = x.hypocenter.as_ref();
            Some(Place {
                origin_ms: x.origin_time_ms,
                lat: h.and_then(|h| h.latitude),
                lon: h.and_then(|h| h.longitude),
            })
        }
        _ => None,
    }
}

/// まとまりの場所:後から来た情報を優先し、欠けた項目は前の情報で補う
fn merged_place(g: &[(u64, &Quake)]) -> Place {
    let last = |f: &dyn Fn(&Place) -> Option<f64>| g.iter().rev().find_map(|(_, q)| f(&place_of(q)));
    Place {
        origin_ms: g.iter().rev().find_map(|(_, q)| place_of(q).origin_ms),
        lat: last(&|p| p.lat),
        lon: last(&|p| p.lon),
    }
}

/// 都道府県ごとの最大震度。観測点の震度を優先し、無ければ集計済みのもの
fn pref_scales(g: &[(u64, &Quake)]) -> Vec<(String, Scale)> {
    let mut max: BTreeMap<&str, Scale> = BTreeMap::new();
    if let Some((_, q)) = g.iter().rev().find(|(_, q)| !q.points.is_empty()) {
        for p in q.points.iter().filter(|p| !p.pref.is_empty()) {
            let e = max.entry(&p.pref).or_insert(p.scale);
            *e = (*e).max(p.scale);
        }
    } else if let Some((_, q)) = g.iter().rev().find(|(_, q)| !q.pref_max.is_empty()) {
        for p in &q.pref_max {
            max.insert(&p.pref, p.scale);
        }
    }
    max.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn summarize(g: &[(u64, &Quake)]) -> QuakeSummary {
    let latest = g[g.len() - 1].1;
    QuakeSummary {
        updated_ms: g.iter().map(|(t, _)| *t).max().unwrap_or(0),
        origin_time: latest.origin_time.clone(),
        origin_ms: merged_place(g).origin_ms,
        hypocenter: g.iter().rev().find_map(|(_, q)| q.hypocenter.clone()),
        max_scale: g.iter().map(|(_, q)| q.max_scale).max().unwrap_or(Scale::UNKNOWN),
        tsunami: latest.domestic_tsunami.clone(),
        pref_scales: pref_scales(g),
        points: g
            .iter()
            .rev()
            .find(|(_, q)| !q.points.is_empty())
            .map(|(_, q)| {
                q.points
                    .iter()
                    .map(|p| Point {
                        addr: p.addr.clone(),
                        is_area: p.is_area,
                        station: p.station.as_ref().map(|s| (s.lat, s.lon, s.area.clone())),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// 地震情報を同じ地震ごとにまとめる (新しい順)
pub fn group_quakes(events: &[Event]) -> Vec<QuakeSummary> {
    let mut quakes: Vec<(u64, &Quake)> = events
        .iter()
        .filter_map(|e| match &e.body {
            EventBody::Quake(q) => Some((e.received_at_ms, q)),
            _ => None,
        })
        .collect();
    quakes.sort_by_key(|(t, _)| *t); // 同時刻なら届いた順
    let mut groups: Vec<Vec<(u64, &Quake)>> = Vec::new();
    for item in quakes {
        let p = place_of(item.1);
        match groups.iter_mut().find(|g| same_quake(&merged_place(g), &p)) {
            Some(g) => g.push(item),
            None => groups.push(vec![item]),
        }
    }
    let mut out: Vec<_> = groups.iter().map(|g| summarize(g)).collect();
    out.sort_by_key(|g| std::cmp::Reverse(g.updated_ms));
    out
}

/// 地震の画面に出す地震 (now はサーバの時計)。無ければ平時。
/// 複数あれば、最大震度の大きい方、同じなら新しい方
pub fn current_quake(groups: &[QuakeSummary], now_ms: u64) -> Option<&QuakeSummary> {
    groups
        .iter()
        .filter(|g| now_ms.saturating_sub(g.updated_ms) <= settle_ms(g.max_scale))
        .max_by_key(|g| (g.max_scale, g.updated_ms))
}

/// サーバの時計とのずれ (ミリ秒)。サーバの時刻から自分の時刻を引く
pub fn clock_offset(server_ms: u64, local_ms: u64) -> i64 {
    server_ms as i64 - local_ms as i64
}

pub fn server_now(local_ms: u64, offset: i64) -> u64 {
    (local_ms as i64 + offset).max(0) as u64
}

/// mixer への BGM の知らせ (平時は流し、地震の画面の間は止める)
pub fn bgm_notice(calm: bool) -> String {
    if calm {
        format!(r#"{{"type":"bgm","play":true,"volume":{BGM_VOLUME}}}"#)
    } else {
        r#"{"type":"bgm","play":false}"#.to_string()
    }
}

// 気象庁の震度配色 (web/src/scale.ts)
pub fn scale_color(s: Scale) -> [u8; 3] {
    match s.0 {
        10 => [0xf2, 0xf2, 0xff],
        20 => [0x00, 0xaa, 0xff],
        30 => [0x00, 0x41, 0xff],
        40 => [0xfa, 0xf5, 0x00],
        45..=47 => [0xff, 0xe6, 0x00],
        50 => [0xff, 0x99, 0x00],
        55 | 57 => [0xff, 0x28, 0x00],
        60 => [0xa5, 0x00, 0x21],
        70 => [0xb4, 0x00, 0x68],
        _ => [0x66, 0x6a, 0x73],
    }
}

/// 背景色に対して読みやすい文字色
pub fn scale_text_color(s: Scale) -> [u8; 3] {
    if matches!(s.0, 10 | 40 | 45 | 46 | 47) {
        [0x11, 0x11, 0x11]
    } else {
        [0xff, 0xff, 0xff]
    }
}

/// 震源の文 (「震源名 / M5.0 / 深さ10km」)
pub fn hypo_text(h: Option<&Hypocenter>) -> String {
    let Some(h) = h else {
        return "震源調査中".into();
    };
    let name = if h.name.is_empty() { "震源不明" } else { &h.name };
    let mut parts = vec![name.to_string()];
    if let Some(m) = h.magnitude {
        parts.push(format!("M{m:.1}"));
    }
    match h.depth_km {
        Some(0) => parts.push("ごく浅い".into()),
        Some(d) => parts.push(format!("深さ{d}km")),
        None => {}
    }
    parts.join(" / ")
}

pub fn tsunami_text(code: &str) -> &'static str {
    match code {
        "None" => "この地震による津波の心配はありません",
        "NonEffective" => "若干の海面変動 (被害の心配なし)",
        "Checking" => "津波の有無を調査中",
        "Watch" => "津波注意報 発表中",
        "Warning" => "津波警報等 発表中",
        _ => "—",
    }
}

/// 生放送で、いま届いた報 (events の末尾) が警戒音を鳴らすか。鳴らすなら音の強さと、読み上げる報の id (末尾の報の id) を返す。
/// events はここまでに届いた報すべて (到着順)。判断は動画の音の規則 (replay の alert_level) と同じ。
pub fn live_alert(events: &[Event], now_ms: u64) -> Option<(AlertLevel, String)> {
    let level = crate::broadcast::replay::sound::alert_level(events, now_ms)?;
    Some((level, events.last()?.id.clone()))
}

/// 発表中の津波予報区の最大の等級 (解除・予報区なしは 0)。web/src/tsunami.ts の maxRank と同じ
fn tsunami_rank(t: &Tsunami) -> u8 {
    if t.cancelled {
        return 0;
    }
    t.areas.iter().map(|a| a.grade as u8).max().unwrap_or(0)
}

/// 警戒音を鳴らさなくても読み上げる報か。津波は、予報が出た・等級が上がった (注意報以上) ときだけ。
/// before はその報より前に届いた報 (到着順)。web/src/tsunami.ts の tsunamiAlert と同じ規則
/// (続報・解除・古い予報の遅れた到着では読まない)
pub fn should_read_without_alert(before: &[Event], ev: &Event) -> bool {
    let EventBody::Tsunami(next) = &ev.body else {
        return false;
    };
    let prev = before
        .iter()
        .filter_map(|e| match &e.body {
            EventBody::Tsunami(t) => Some(t),
            _ => None,
        })
        .reduce(|cur, t| if t.issued_at < cur.issued_at { cur } else { t });
    if prev.is_some_and(|p| next.issued_at < p.issued_at) {
        return false;
    }
    let r = tsunami_rank(next);
    r >= TsunamiGrade::Watch as u8 && r > prev.map_or(0, tsunami_rank)
}

/// 読み上げの声の URL (id は URL 用に % で符号化する)
pub fn voice_url(base: &str, id: &str) -> String {
    let mut enc = String::new();
    for b in id.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            enc.push(b as char);
        } else {
            enc.push_str(&format!("%{b:02X}"));
        }
    }
    format!("{base}/api/tts/event/{enc}")
}

/// mixer への警戒音の知らせ
pub fn alert_notice(level: AlertLevel) -> String {
    format!(
        r#"{{"type":"alert","level":"{}"}}"#,
        format!("{level:?}").to_lowercase()
    )
}

/// mixer への読み上げの知らせ
pub fn voice_notice(url: &str) -> String {
    serde_json::json!({"type": "voice", "url": url}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quake::{ObservationPoint, QuakeInfoType};

    const T0: i64 = 1_790_000_000_000;

    // ---- live_alert ----
    use crate::quake::{Eew, Tsunami};

    const LT: u64 = T0 as u64;

    fn eew(event_id: &str, serial: u32, recv: u64, warning: bool, scale: Scale) -> Event {
        Event {
            id: format!("{event_id}-{serial}"),
            source: "wolfx".into(),
            received_at_ms: recv,
            body: EventBody::Eew(Eew {
                event_id: event_id.into(),
                serial: serial.to_string(),
                cancelled: false,
                test: false,
                warning,
                issued_at: String::new(),
                origin_time: None,
                origin_time_ms: Some(T0),
                hypocenter: None,
                areas: vec![],
                pref_max: vec![],
                max_scale: scale,
            }),
        }
    }

    fn live_quake(recv: u64) -> Event {
        let mut e = quake("q", recv, T0 - 20_000, Some((35.68, 139.76)), Scale::S4);
        if let EventBody::Quake(q) = &mut e.body {
            q.info_type = QuakeInfoType::ScalePrompt;
        }
        e
    }

    #[test]
    fn live_alert_rings_for_the_first_eew_report_with_that_events_id() {
        let ev = [eew("E", 1, LT, false, Scale::S4)];
        assert_eq!(live_alert(&ev, LT), Some((AlertLevel::Medium, "E-1".to_string())));
    }

    #[test]
    fn live_alert_is_silent_for_a_second_report_with_the_same_max_scale() {
        let ev = [
            eew("E", 1, LT, false, Scale::S4),
            eew("E", 2, LT + 1_000, false, Scale::S4),
        ];
        assert_eq!(live_alert(&ev, LT + 1_000), None);
    }

    #[test]
    fn live_alert_rings_when_the_max_scale_rises() {
        let ev = [
            eew("E", 1, LT, false, Scale::S2),
            eew("E", 2, LT + 1_000, false, Scale::S4),
        ];
        assert_eq!(
            live_alert(&ev, LT + 1_000),
            Some((AlertLevel::Medium, "E-2".to_string()))
        );
    }

    #[test]
    fn live_alert_is_silent_for_a_cancelled_eew() {
        let mut e = eew("E", 1, LT, false, Scale::S4);
        if let EventBody::Eew(x) = &mut e.body {
            x.cancelled = true;
        }
        assert_eq!(live_alert(&[e], LT), None);
    }

    #[test]
    fn live_alert_is_silent_for_a_test_eew_from_p2pquake() {
        // 訓練報 (test = true)。alert_level は source を見ず、test フラグだけで黙る
        let mut e = eew("E", 1, LT, true, Scale::S5_LOWER);
        e.source = "p2pquake".into();
        if let EventBody::Eew(x) = &mut e.body {
            x.test = true;
        }
        assert_eq!(live_alert(&[e], LT), None);
    }

    #[test]
    fn live_alert_rings_for_a_first_quake_report_without_a_prior_eew() {
        let ev = [live_quake(LT)];
        assert_eq!(live_alert(&ev, LT), Some((AlertLevel::Medium, "q".to_string())));
    }

    fn tsunami(id: &str, issued: &str, cancelled: bool, grades: &[TsunamiGrade]) -> Event {
        Event {
            id: id.into(),
            source: "test".into(),
            received_at_ms: LT,
            body: EventBody::Tsunami(Tsunami {
                cancelled,
                issued_at: issued.into(),
                areas: grades
                    .iter()
                    .map(|g| crate::quake::model::TsunamiArea {
                        name: "宮城県".into(),
                        grade: *g,
                        immediate: false,
                        first_height: None,
                        max_height: None,
                    })
                    .collect(),
            }),
        }
    }

    #[test]
    fn a_tsunami_is_read_only_when_it_first_appears_or_rises() {
        use TsunamiGrade::*;
        let watch = tsunami("a", "2026-01-01T00:00", false, &[Watch]);
        let watch2 = tsunami("b", "2026-01-01T00:10", false, &[Watch]);
        let warn = tsunami("c", "2026-01-01T00:20", false, &[Watch, Warning]);
        let major = tsunami("d", "2026-01-01T00:30", false, &[MajorWarning]);
        let cancel = tsunami("e", "2026-01-01T00:40", true, &[]);
        assert!(should_read_without_alert(&[], &watch)); // 初めての注意報
        assert!(!should_read_without_alert(std::slice::from_ref(&watch), &watch2)); // 同じ等級の続報
        assert!(should_read_without_alert(std::slice::from_ref(&watch), &warn)); // 警報へ上がった
        assert!(!should_read_without_alert(&[watch.clone(), warn.clone()], &watch2)); // 下がった
        assert!(should_read_without_alert(std::slice::from_ref(&warn), &major));
        assert!(!should_read_without_alert(std::slice::from_ref(&watch), &cancel)); // 解除
        let again = tsunami("f", "2026-01-01T00:50", false, &[Watch]);
        assert!(should_read_without_alert(&[watch.clone(), cancel.clone()], &again)); // 解除のあとの再発表
        assert!(!should_read_without_alert(&[], &tsunami("u", "t", false, &[Unknown])));
        // 等級不明
    }

    #[test]
    fn a_late_older_tsunami_report_is_not_read() {
        use TsunamiGrade::*;
        let old = tsunami("a", "2026-01-01T00:00", false, &[Warning]);
        let new = tsunami("b", "2026-01-01T00:10", false, &[Watch]);
        assert!(!should_read_without_alert(&[new], &old));
    }

    #[test]
    fn a_quake_is_not_read_without_an_alert() {
        assert!(!should_read_without_alert(&[], &live_quake(LT)));
    }

    #[test]
    fn live_alert_is_silent_for_a_tsunami_event() {
        // alert_level は津波情報を鳴らさない (Eew と Quake だけ)
        let e = Event {
            id: "t".into(),
            source: "p2pquake".into(),
            received_at_ms: LT,
            body: EventBody::Tsunami(Tsunami {
                cancelled: false,
                issued_at: String::new(),
                areas: vec![],
            }),
        };
        assert_eq!(live_alert(&[e], LT), None);
    }

    fn quake(id: &str, recv: u64, origin_ms: i64, at: Option<(f64, f64)>, scale: Scale) -> Event {
        Event {
            id: id.into(),
            source: "test".into(),
            received_at_ms: recv,
            body: EventBody::Quake(Quake {
                info_type: QuakeInfoType::Destination,
                origin_time: String::new(),
                origin_time_ms: Some(origin_ms),
                issued_at: String::new(),
                hypocenter: at.map(|(la, lo)| Hypocenter {
                    name: format!("{la},{lo}"),
                    latitude: Some(la),
                    longitude: Some(lo),
                    depth_km: Some(10),
                    magnitude: Some(4.0),
                }),
                max_scale: scale,
                domestic_tsunami: "None".into(),
                points: vec![],
                pref_max: vec![],
                comment: String::new(),
            }),
        }
    }

    fn with_points(mut e: Event, pts: &[(&str, Scale)]) -> Event {
        if let EventBody::Quake(q) = &mut e.body {
            q.points = pts
                .iter()
                .map(|(p, s)| ObservationPoint {
                    pref: p.to_string(),
                    addr: "x".into(),
                    is_area: false,
                    scale: *s,
                    station: None,
                })
                .collect();
        }
        e
    }

    #[test]
    fn minor_quakes_are_shown_for_a_minute_and_others_for_three() {
        let t = T0 as u64;
        for (scale, limit) in [
            (Scale::S2, 60_000),
            (Scale::S1, 60_000),
            (Scale::S3, 180_000),
            (Scale::UNKNOWN, 180_000),
        ] {
            let g = group_quakes(&[quake("a", t, T0, Some((35.0, 139.0)), scale)]);
            assert!(current_quake(&g, t + limit).is_some(), "{scale:?} at the limit");
            assert!(current_quake(&g, t + limit + 1).is_none(), "{scale:?} after the limit");
        }
    }

    #[test]
    fn the_screen_goes_calm_quake_calm() {
        let t = T0 as u64;
        assert!(current_quake(&[], t).is_none());
        let g = group_quakes(&[quake("a", t, T0, Some((35.0, 139.0)), Scale::S4)]);
        assert!(current_quake(&g, t + 10_000).is_some());
        assert!(current_quake(&g, t + 200_000).is_none());
    }

    #[test]
    fn same_quake_needs_90_seconds_and_200_km() {
        let p = |ms, at: Option<(f64, f64)>| Place {
            origin_ms: Some(ms),
            lat: at.map(|a| a.0),
            lon: at.map(|a| a.1),
        };
        let tokyo = Some((35.68, 139.76));
        assert!(same_quake(&p(T0, tokyo), &p(T0 + 90_000, tokyo)));
        assert!(!same_quake(&p(T0, tokyo), &p(T0 + 90_001, tokyo)));
        // 東京と大阪 (約 400km) は別の地震、東京と横浜 (約 30km) は同じ
        assert!(!same_quake(&p(T0, tokyo), &p(T0, Some((34.69, 135.5)))));
        assert!(same_quake(&p(T0, tokyo), &p(T0, Some((35.44, 139.64)))));
        // 震源が無ければ発生時刻だけで決める。発生時刻が無ければ別
        assert!(same_quake(&p(T0, None), &p(T0 + 30_000, tokyo)));
        let none = Place {
            origin_ms: None,
            lat: None,
            lon: None,
        };
        assert!(!same_quake(&none, &p(T0, None)));
    }

    #[test]
    fn reports_of_one_quake_are_merged_and_the_latest_wins() {
        let t = T0 as u64;
        let events = [
            quake("1", t, T0, None, Scale::S3),
            with_points(
                quake("2", t + 60_000, T0, Some((35.0, 139.0)), Scale::S4),
                &[("東京都", Scale::S3), ("東京都", Scale::S4), ("神奈川県", Scale::S2)],
            ),
            quake("3", t + 5_000_000, T0 + 3_600_000, Some((43.0, 141.0)), Scale::S1), // 1 時間後の別の地震
        ];
        let g = group_quakes(&events);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].max_scale, Scale::S1); // 新しい順
        assert_eq!(g[1].max_scale, Scale::S4);
        assert_eq!(g[1].updated_ms, t + 60_000);
        assert_eq!(g[1].hypocenter.as_ref().unwrap().latitude, Some(35.0)); // 震源の無い最初の情報で消えない
        assert_eq!(
            g[1].pref_scales,
            [("東京都".to_string(), Scale::S4), ("神奈川県".to_string(), Scale::S2)]
        );
    }

    #[test]
    fn the_biggest_quake_wins_and_ties_go_to_the_newer_one() {
        let t = T0 as u64;
        let far = 3_600_000;
        let events = [
            quake("a", t, T0, Some((35.0, 139.0)), Scale::S5_LOWER),
            quake("b", t + 5_000, T0 + far, Some((43.0, 141.0)), Scale::S3),
            quake("c", t + 6_000, T0 + 2 * far, Some((33.0, 130.0)), Scale::S5_LOWER),
        ];
        let g = group_quakes(&events);
        assert_eq!(current_quake(&g, t + 10_000).unwrap().updated_ms, t + 6_000);
        // 5 弱は 3 分で外れ、残った震度 3 が出る
        let events = [
            events[0].clone(),
            quake("b", t + 170_000, T0 + far, Some((43.0, 141.0)), Scale::S3),
        ];
        let g = group_quakes(&events);
        assert_eq!(current_quake(&g, t + 180_001).unwrap().max_scale, Scale::S3);
    }

    #[test]
    fn the_local_clock_follows_the_server() {
        let off = clock_offset(1_000_500, 1_000_000);
        assert_eq!(off, 500);
        assert_eq!(server_now(2_000_000, off), 2_000_500);
        assert_eq!(server_now(2_000_000, clock_offset(999_000, 1_000_000)), 1_999_000);
    }

    #[test]
    fn bgm_notices_are_understood_by_the_mixer() {
        use crate::broadcast::mixer::Notice;
        assert_eq!(
            serde_json::from_str::<Notice>(&bgm_notice(true)).unwrap(),
            Notice::Bgm {
                play: true,
                volume: Some(0.4)
            }
        );
        assert_eq!(
            serde_json::from_str::<Notice>(&bgm_notice(false)).unwrap(),
            Notice::Bgm {
                play: false,
                volume: None
            }
        );
    }

    #[test]
    fn colors_and_texts_match_the_page() {
        assert_eq!(scale_color(Scale::S4), [0xfa, 0xf5, 0x00]);
        assert_eq!(scale_color(Scale::S7), [0xb4, 0x00, 0x68]);
        assert_eq!(scale_text_color(Scale::S4), [0x11, 0x11, 0x11]);
        assert_eq!(scale_text_color(Scale::S5_UPPER), [0xff, 0xff, 0xff]);
        let h = Hypocenter {
            name: "千葉県東方沖".into(),
            latitude: None,
            longitude: None,
            depth_km: Some(0),
            magnitude: Some(5.04),
        };
        assert_eq!(hypo_text(Some(&h)), "千葉県東方沖 / M5.0 / ごく浅い");
        assert_eq!(hypo_text(None), "震源調査中");
    }

    #[test]
    fn voice_url_encodes_slash_and_space() {
        assert_eq!(voice_url("http://h", "a/b c"), "http://h/api/tts/event/a%2Fb%20c");
    }

    #[test]
    fn alert_notice_is_lowercase_level() {
        assert_eq!(alert_notice(AlertLevel::Strong), r#"{"type":"alert","level":"strong"}"#);
    }
}
