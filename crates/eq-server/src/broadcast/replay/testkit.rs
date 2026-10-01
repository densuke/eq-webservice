//! テストで使う、報の作り方。

use crate::quake::{Eew, Event, EventBody, Hypocenter, Quake, QuakeInfoType, Scale};

/// 地震の発生時刻 (epoch ミリ秒)
pub const T0: i64 = 1_790_000_000_000;
pub const TOKYO: (f64, f64) = (35.68, 139.76);
pub const OSAKA: (f64, f64) = (34.69, 135.5);

fn hypo(at: (f64, f64)) -> Hypocenter {
    Hypocenter {
        name: "x".into(),
        latitude: Some(at.0),
        longitude: Some(at.1),
        depth_km: Some(10),
        magnitude: Some(5.0),
    }
}

/// 緊急地震速報の 1 報 (event_id の地震の第 serial 報)
pub fn eew(event_id: &str, serial: u32, recv: u64, origin: i64, warning: bool, scale: Scale, at: (f64, f64)) -> Event {
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
            origin_time_ms: Some(origin),
            hypocenter: Some(hypo(at)),
            areas: vec![],
            pref_max: vec![],
            max_scale: scale,
        }),
    }
}

/// 地震情報の 1 報。at が None なら震度速報のように震源が無い
pub fn quake(recv: u64, info: QuakeInfoType, origin: i64, scale: Scale, at: Option<(f64, f64)>) -> Event {
    Event {
        id: format!("q-{recv}"),
        source: "p2pquake".into(),
        received_at_ms: recv,
        body: EventBody::Quake(Quake {
            info_type: info,
            origin_time: String::new(),
            origin_time_ms: Some(origin),
            issued_at: String::new(),
            hypocenter: at.map(hypo),
            max_scale: scale,
            domestic_tsunami: String::new(),
            points: vec![],
            pref_max: vec![],
            comment: String::new(),
        }),
    }
}

/// 訓練報にする
pub fn as_test(mut e: Event) -> Event {
    if let EventBody::Eew(x) = &mut e.body {
        x.test = true;
    }
    e
}

/// 取り消し報にする
pub fn as_cancelled(mut e: Event) -> Event {
    if let EventBody::Eew(x) = &mut e.body {
        x.cancelled = true;
    }
    e
}
