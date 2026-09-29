//! Wolfx Open API の JMA 緊急地震速報 (予報・警報) からの変換。
//!
//! P2P地震情報は警報しか配信しないため、予報はこちらから受け取る。
//! 仕様: https://wolfx.jp/docs/open-api (WebSocket: wss://ws-api.wolfx.jp/jma_eew)

use serde::Deserialize;

use crate::quake::{aggregate_pref_max, area::area_pref, jst, Eew, EewArea, Event, EventBody, Hypocenter, Scale};

pub const SOURCE: &str = "wolfx";

#[derive(Deserialize)]
struct Raw {
    #[serde(rename = "EventID")]
    event_id: String,
    #[serde(rename = "Serial")]
    serial: u32,
    #[serde(rename = "AnnouncedTime")]
    announced_time: String,
    #[serde(rename = "OriginTime")]
    origin_time: Option<String>,
    #[serde(rename = "Hypocenter", default)]
    hypocenter: String,
    #[serde(rename = "Latitude")]
    latitude: Option<f64>,
    #[serde(rename = "Longitude")]
    longitude: Option<f64>,
    #[serde(rename = "Magnitude")]
    magnitude: Option<f64>,
    #[serde(rename = "Depth")]
    depth: Option<f64>,
    #[serde(rename = "MaxIntensity", default)]
    max_intensity: String,
    #[serde(rename = "WarnArea", default)]
    warn_area: Vec<RawArea>,
    #[serde(rename = "isTraining", default)]
    is_training: bool,
    #[serde(rename = "isWarn", default)]
    is_warn: bool,
    #[serde(rename = "isCancel", default)]
    is_cancel: bool,
}

#[derive(Deserialize)]
struct RawArea {
    #[serde(rename = "Chiiki", default)]
    chiiki: String,
    /// 最大震度
    #[serde(rename = "Shindo1", default)]
    shindo1: String,
    /// 最小震度
    #[serde(rename = "Shindo2", default)]
    shindo2: String,
    /// 主要動の到達予想時刻 (HHMMSS。予測なしは "//////")
    #[serde(rename = "Time", default)]
    time: String,
    #[serde(rename = "Arrive", default)]
    arrive: String,
}

/// 1 メッセージを変換する。緊急地震速報以外 (heartbeat など) は `Ok(None)`。
pub fn parse(json: &str) -> Result<Option<Event>, serde_json::Error> {
    let value: serde_json::Value = serde_json::from_str(json)?;
    // type は WebSocket のみ。jma_eew 以外 (heartbeat, pong) は捨てる
    if value
        .get("type")
        .and_then(|t| t.as_str())
        .is_some_and(|t| t != "jma_eew")
    {
        return Ok(None);
    }
    Ok(Some(to_event(serde_json::from_value(value)?)))
}

fn to_event(r: Raw) -> Event {
    let origin_date = r
        .origin_time
        .as_deref()
        .and_then(|t| t.get(..10))
        .unwrap_or("")
        .to_string();
    let areas: Vec<EewArea> = r
        .warn_area
        .into_iter()
        .map(|a| EewArea {
            pref: area_pref(&a.chiiki).to_string(),
            scale_from: Scale::parse(&a.shindo2).unwrap_or(Scale::UNKNOWN),
            scale_to: Scale::parse(&a.shindo1),
            arrival_time: arrival_time(&origin_date, &a.time),
            arrived: a.arrive.contains("既に到達"),
            name: a.chiiki,
        })
        .collect();
    let pref_max = aggregate_pref_max(
        areas
            .iter()
            .map(|a| (a.pref.as_str(), a.scale_to.unwrap_or(a.scale_from))),
    );
    let hypocenter = (!r.hypocenter.is_empty()).then(|| Hypocenter {
        name: r.hypocenter,
        latitude: r.latitude,
        longitude: r.longitude,
        depth_km: r.depth.filter(|d| *d >= 0.0).map(|d| d.round() as i32),
        magnitude: r.magnitude,
    });
    Event {
        id: format!("wolfx-{}-{}", r.event_id, r.serial),
        source: SOURCE.to_string(),
        received_at_ms: 0,
        body: EventBody::Eew(Eew {
            event_id: r.event_id,
            serial: r.serial.to_string(),
            cancelled: r.is_cancel,
            test: r.is_training,
            warning: r.is_warn,
            issued_at: r.announced_time,
            origin_time_ms: r.origin_time.as_deref().and_then(jst::parse_ms),
            origin_time: r.origin_time,
            hypocenter,
            areas,
            pref_max,
            max_scale: Scale::parse(&r.max_intensity).unwrap_or(Scale::UNKNOWN),
        }),
    }
}

/// "044525" → "2026/09/29 04:45:25" (発生日の日付を使う)
fn arrival_time(date: &str, hhmmss: &str) -> Option<String> {
    let valid = date.len() == 10 && hhmmss.len() == 6 && hhmmss.bytes().all(|b| b.is_ascii_digit());
    valid.then(|| format!("{date} {}:{}:{}", &hhmmss[..2], &hhmmss[2..4], &hhmmss[4..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORECAST: &str = r#"{
      "type": "jma_eew", "Title": "緊急地震速報（予報）", "EventID": "20260929044513", "Serial": 14,
      "AnnouncedTime": "2026/09/29 04:45:58", "OriginTime": "2026/09/29 04:45:05",
      "Hypocenter": "茨城県南部", "Latitude": 36.1, "Longitude": 140.0, "Magunitude": 5.2, "Magnitude": 5.2,
      "Depth": 50, "MaxIntensity": "4",
      "WarnArea": [
        {"Chiiki": "茨城県南部", "Shindo1": "4", "Shindo2": "4", "Time": "//////", "Type": "予報", "Arrive": "既に到達と予測"},
        {"Chiiki": "千葉県北西部", "Shindo1": "4", "Shindo2": "3", "Time": "//////", "Type": "予報", "Arrive": "既に到達と予測"},
        {"Chiiki": "栃木県南部", "Shindo1": "4", "Shindo2": "4", "Time": "044525", "Type": "予報", "Arrive": "主要動到達時刻の予測なし（PLUM 法による予測）"}
      ],
      "isSea": false, "isTraining": false, "isAssumption": false, "isWarn": false, "isFinal": true, "isCancel": false
    }"#;

    #[test]
    fn converts_forecast() {
        let ev = parse(FORECAST).unwrap().unwrap();
        assert_eq!(ev.id, "wolfx-20260929044513-14");
        assert_eq!(ev.source, SOURCE);
        let EventBody::Eew(e) = ev.body else { panic!() };
        assert!(!e.warning && !e.cancelled && !e.test);
        assert_eq!(e.event_id, "20260929044513");
        assert_eq!(e.serial, "14");
        assert_eq!(e.max_scale, Scale::S4);
        assert_eq!(e.origin_time_ms, jst::parse_ms("2026/09/29 04:45:05"));
        let h = e.hypocenter.unwrap();
        assert_eq!(
            (h.name.as_str(), h.latitude, h.depth_km),
            ("茨城県南部", Some(36.1), Some(50))
        );
        assert_eq!(e.areas[1].pref, "千葉県");
        assert_eq!(
            (e.areas[1].scale_from, e.areas[1].scale_to),
            (Scale::S3, Some(Scale::S4))
        );
        assert!(e.areas[0].arrived && !e.areas[2].arrived);
        assert_eq!(e.areas[2].arrival_time.as_deref(), Some("2026/09/29 04:45:25"));
        assert_eq!(e.areas[0].arrival_time, None);
        let prefs: Vec<_> = e.pref_max.iter().map(|p| p.pref.as_str()).collect();
        assert_eq!(prefs, ["茨城県", "千葉県", "栃木県"]);
    }

    #[test]
    fn warning_and_cancel_flags() {
        let warn = FORECAST.replace(r#""isWarn": false"#, r#""isWarn": true"#);
        let EventBody::Eew(e) = parse(&warn).unwrap().unwrap().body else {
            panic!()
        };
        assert!(e.warning);
        let cancel = FORECAST.replace(r#""isCancel": false"#, r#""isCancel": true"#);
        let EventBody::Eew(e) = parse(&cancel).unwrap().unwrap().body else {
            panic!()
        };
        assert!(e.cancelled);
    }

    #[test]
    fn ignores_heartbeat_and_pong() {
        assert!(parse(r#"{"type":"heartbeat","ver":1,"id":"x","timestamp":"1"}"#)
            .unwrap()
            .is_none());
        assert!(parse(r#"{"type":"pong","timestamp":"1"}"#).unwrap().is_none());
    }
}
