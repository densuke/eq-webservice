//! P2P地震情報 JSON API v2 (https://www.p2pquake.net/develop/json_api_v2/) の変換。
//!
//! WebSocket (`wss://api.p2pquake.net/v2/ws`) と履歴 API (`/v2/history`) の
//! どちらの JSON も受け付ける (ID が前者は `_id`、後者は `id`)。

use serde::Deserialize;

use crate::jst;
use crate::model::*;
use crate::scale::Scale;

pub const SOURCE: &str = "p2pquake";

/// 地震情報
pub const CODE_QUAKE: u32 = 551;
/// 津波予報
pub const CODE_TSUNAMI: u32 = 552;
/// 緊急地震速報 発表検出
pub const CODE_EEW_DETECTION: u32 = 554;
/// 緊急地震速報 (警報)
pub const CODE_EEW: u32 = 556;

/// 地震感知情報の評価結果
pub const CODE_USERQUAKE_EVALUATION: u32 = 9611;

/// このクレートが扱う code。履歴 API のクエリにも使う (地震感知情報は過去のものを取り込まない)。
pub const HANDLED_CODES: [u32; 4] = [CODE_QUAKE, CODE_TSUNAMI, CODE_EEW_DETECTION, CODE_EEW];

#[derive(Debug)]
pub enum ParseError {
    Json(serde_json::Error),
    MissingId,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Json(e) => write!(f, "invalid JSON: {e}"),
            ParseError::MissingId => write!(f, "message has no id"),
        }
    }
}

impl std::error::Error for ParseError {}

impl From<serde_json::Error> for ParseError {
    fn from(e: serde_json::Error) -> Self {
        ParseError::Json(e)
    }
}

#[derive(Deserialize)]
struct Envelope {
    code: u32,
    #[serde(alias = "_id")]
    id: Option<String>,
}

/// 1 メッセージを変換する。扱わない code (ピア情報など) は `Ok(None)`。
pub fn parse(json: &str) -> Result<Option<Event>, ParseError> {
    let value: serde_json::Value = serde_json::from_str(json)?;
    parse_value(value)
}

pub fn parse_value(value: serde_json::Value) -> Result<Option<Event>, ParseError> {
    let env: Envelope = serde_json::from_value(value.clone())?;
    if !HANDLED_CODES.contains(&env.code) && env.code != CODE_USERQUAKE_EVALUATION {
        return Ok(None);
    }
    let id = env.id.ok_or(ParseError::MissingId)?;
    let body = match env.code {
        CODE_QUAKE => EventBody::Quake(serde_json::from_value::<RawQuake>(value)?.into()),
        CODE_TSUNAMI => EventBody::Tsunami(serde_json::from_value::<RawTsunami>(value)?.into()),
        CODE_EEW_DETECTION => {
            let raw: RawEewDetection = serde_json::from_value(value)?;
            EventBody::EewDetection(EewDetection {
                detection_type: raw.r#type,
            })
        }
        CODE_EEW => EventBody::Eew(serde_json::from_value::<RawEew>(value)?.into()),
        CODE_USERQUAKE_EVALUATION => {
            let raw: RawUserquakeEvaluation = serde_json::from_value(value)?;
            // 信頼度 0 は P2P地震情報でも表示しない評価
            if raw.confidence <= 0.0 {
                return Ok(None);
            }
            EventBody::Userquake(raw.into())
        }
        _ => unreachable!(),
    };
    Ok(Some(Event {
        id,
        source: SOURCE.to_string(),
        received_at_ms: 0,
        body,
    }))
}

// ---- 551: 地震情報 ----

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawQuake {
    issue: RawIssue,
    earthquake: RawEarthquake,
    #[serde(default)]
    points: Vec<RawPoint>,
    #[serde(default)]
    comments: RawComments,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawIssue {
    #[serde(default)]
    time: String,
    #[serde(default)]
    r#type: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEarthquake {
    #[serde(default)]
    time: String,
    hypocenter: Option<RawHypocenter>,
    #[serde(default = "unknown_scale")]
    max_scale: i32,
    #[serde(default)]
    domestic_tsunami: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawHypocenter {
    #[serde(default)]
    name: String,
    latitude: Option<f64>,
    longitude: Option<f64>,
    depth: Option<f64>,
    magnitude: Option<f64>,
}

#[derive(Deserialize)]
struct RawUserquakeEvaluation {
    count: u32,
    confidence: f64,
    #[serde(default)]
    started_at: String,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    area_confidences: std::collections::BTreeMap<String, RawAreaConfidence>,
}

#[derive(Deserialize)]
struct RawAreaConfidence {
    #[serde(default)]
    confidence: f64,
    #[serde(default)]
    count: u32,
}

impl From<RawUserquakeEvaluation> for Userquake {
    fn from(r: RawUserquakeEvaluation) -> Self {
        Userquake {
            started_at: r.started_at,
            updated_at: r.updated_at,
            count: r.count,
            confidence: r.confidence,
            areas: r
                .area_confidences
                .into_iter()
                .filter_map(|(code, a)| {
                    Some(UserquakeArea {
                        code: code.parse().ok()?,
                        count: a.count,
                        confidence: a.confidence,
                    })
                })
                .collect(),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPoint {
    #[serde(default)]
    pref: String,
    #[serde(default)]
    addr: String,
    #[serde(default)]
    is_area: bool,
    scale: i32,
    /// 過去の記録のデモ用の拡張 (P2P地震情報には無い): 観測点の位置と細分区域
    #[serde(default)]
    station: Option<StationPos>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawComments {
    #[serde(default)]
    free_form_comment: String,
}

fn unknown_scale() -> i32 {
    -1
}

impl From<RawHypocenter> for Hypocenter {
    fn from(h: RawHypocenter) -> Self {
        // 不明値: 緯度経度 -200、深さ -1、マグニチュード -1
        let lat = h.latitude.filter(|v| (-90.0..=90.0).contains(v));
        let lon = h.longitude.filter(|v| (-180.0..=180.0).contains(v));
        let (latitude, longitude) = match (lat, lon) {
            (Some(a), Some(b)) => (Some(a), Some(b)),
            _ => (None, None),
        };
        Hypocenter {
            name: h.name,
            latitude,
            longitude,
            depth_km: h.depth.filter(|d| *d >= 0.0).map(|d| d.round() as i32),
            magnitude: h.magnitude.filter(|m| *m >= 0.0),
        }
    }
}

fn quake_info_type(s: &str) -> QuakeInfoType {
    match s {
        "ScalePrompt" => QuakeInfoType::ScalePrompt,
        "Destination" => QuakeInfoType::Destination,
        "ScaleAndDestination" => QuakeInfoType::ScaleAndDestination,
        "DetailScale" => QuakeInfoType::DetailScale,
        "Foreign" => QuakeInfoType::Foreign,
        _ => QuakeInfoType::Other,
    }
}

impl From<RawQuake> for Quake {
    fn from(r: RawQuake) -> Self {
        let points: Vec<ObservationPoint> = r
            .points
            .into_iter()
            .map(|p| ObservationPoint {
                pref: p.pref,
                addr: p.addr,
                is_area: p.is_area,
                scale: Scale(p.scale),
                station: p.station,
            })
            .collect();
        let pref_max = aggregate_pref_max(points.iter().map(|p| (p.pref.as_str(), p.scale)));
        // 震源情報のみ (Destination) は震源名が空・緯度経度が不明のことがある
        let hypocenter = r
            .earthquake
            .hypocenter
            .map(Hypocenter::from)
            .filter(|h| !h.name.is_empty() || h.latitude.is_some() || h.magnitude.is_some());
        Quake {
            info_type: quake_info_type(&r.issue.r#type),
            origin_time_ms: jst::parse_ms(&r.earthquake.time),
            origin_time: r.earthquake.time,
            issued_at: r.issue.time,
            hypocenter,
            max_scale: Scale(r.earthquake.max_scale),
            domestic_tsunami: r.earthquake.domestic_tsunami,
            points,
            pref_max,
            comment: r.comments.free_form_comment,
        }
    }
}

// ---- 552: 津波予報 ----

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTsunami {
    #[serde(default)]
    cancelled: bool,
    #[serde(default)]
    issue: RawIssue,
    #[serde(default)]
    areas: Vec<RawTsunamiArea>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTsunamiArea {
    #[serde(default)]
    name: String,
    #[serde(default)]
    grade: String,
    #[serde(default)]
    immediate: bool,
    first_height: Option<RawFirstHeight>,
    max_height: Option<RawMaxHeight>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawFirstHeight {
    arrival_time: Option<String>,
    condition: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMaxHeight {
    description: Option<String>,
}

impl From<RawTsunami> for Tsunami {
    fn from(r: RawTsunami) -> Self {
        let mut areas: Vec<TsunamiArea> = r
            .areas
            .into_iter()
            .map(|a| TsunamiArea {
                grade: match a.grade.as_str() {
                    "MajorWarning" => TsunamiGrade::MajorWarning,
                    "Warning" => TsunamiGrade::Warning,
                    "Watch" => TsunamiGrade::Watch,
                    _ => TsunamiGrade::Unknown,
                },
                name: a.name,
                immediate: a.immediate,
                first_height: a.first_height.and_then(|f| f.condition.or(f.arrival_time)),
                max_height: a.max_height.and_then(|m| m.description),
            })
            .collect();
        areas.sort_by_key(|a| std::cmp::Reverse(a.grade));
        Tsunami {
            cancelled: r.cancelled,
            issued_at: r.issue.time,
            areas,
        }
    }
}

// ---- 554: 緊急地震速報 発表検出 ----

#[derive(Deserialize)]
struct RawEewDetection {
    #[serde(default)]
    r#type: String,
}

// ---- 556: 緊急地震速報 (警報) ----

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEew {
    #[serde(default)]
    test: bool,
    #[serde(default)]
    cancelled: bool,
    #[serde(default)]
    issue: RawEewIssue,
    earthquake: Option<RawEewEarthquake>,
    #[serde(default)]
    areas: Vec<RawEewArea>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawEewIssue {
    #[serde(default)]
    time: String,
    #[serde(default)]
    event_id: String,
    #[serde(default)]
    serial: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEewEarthquake {
    origin_time: Option<String>,
    hypocenter: Option<RawHypocenter>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEewArea {
    #[serde(default)]
    pref: String,
    #[serde(default)]
    name: String,
    #[serde(default = "unknown_scale")]
    scale_from: i32,
    #[serde(default = "unknown_scale")]
    scale_to: i32,
    /// "10" 未到達 / "11" 既に到達と予測 / "19" 到達予想なし (PLUM 法など)
    #[serde(default)]
    kind_code: String,
    arrival_time: Option<String>,
}

impl From<RawEew> for Eew {
    fn from(r: RawEew) -> Self {
        let areas: Vec<EewArea> = r
            .areas
            .into_iter()
            .map(|a| EewArea {
                // pref は「北海道道北」のような地方名のことがあるので、区域名から都道府県を求める
                pref: Some(crate::area::area_pref(&a.name))
                    .filter(|p| !p.is_empty())
                    .map_or(a.pref, str::to_string),
                name: a.name,
                scale_from: Scale(a.scale_from),
                // 99 は「〜程度以上」
                scale_to: (a.scale_to != 99).then_some(Scale(a.scale_to)),
                arrival_time: a.arrival_time,
                arrived: a.kind_code == "11",
            })
            .collect();
        let pref_max = aggregate_pref_max(areas.iter().map(|a| (a.pref.as_str(), a.scale_from)));
        let max_scale = areas
            .iter()
            .map(|a| a.scale_to.filter(|s| s.is_known()).unwrap_or(a.scale_from))
            .max()
            .unwrap_or(Scale::UNKNOWN);
        let (origin_time, hypocenter) = match r.earthquake {
            Some(e) => (e.origin_time, e.hypocenter.map(Hypocenter::from)),
            None => (None, None),
        };
        Eew {
            event_id: r.issue.event_id,
            serial: r.issue.serial,
            cancelled: r.cancelled,
            test: r.test,
            warning: true,
            issued_at: r.issue.time,
            origin_time_ms: origin_time.as_deref().and_then(jst::parse_ms),
            origin_time,
            hypocenter,
            areas,
            pref_max,
            max_scale,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUAKE: &str = r#"{
      "code": 551, "_id": "abc",
      "comments": {"freeFormComment": ""},
      "earthquake": {
        "domesticTsunami": "None", "foreignTsunami": "Unknown",
        "hypocenter": {"depth": 10, "latitude": 37.4, "longitude": 139.4, "magnitude": 4.5, "name": "福島県会津"},
        "maxScale": 30, "time": "2026/09/28 16:24:00"
      },
      "issue": {"correct": "None", "source": "気象庁", "time": "2026/09/28 16:26:56", "type": "DetailScale"},
      "points": [
        {"addr": "南会津町界", "isArea": false, "pref": "福島県", "scale": 30},
        {"addr": "日光市中宮祠", "isArea": false, "pref": "栃木県", "scale": 20},
        {"addr": "福島金山町川口", "isArea": false, "pref": "福島県", "scale": 10}
      ]
    }"#;

    #[test]
    fn parses_userquake_evaluation_and_skips_hidden_ones() {
        let json = r#"{"code":9611,"id":"u1","count":12,"confidence":0.97015,
          "started_at":"2026/09/29 07:33:29.873","updated_at":"2026/09/29 07:33:41.100","time":"2026/09/29 07:33:42.000",
          "area_confidences":{"270":{"confidence":0.85,"count":8,"display":"A"},"275":{"confidence":0.3,"count":4,"display":"D"}}}"#;
        let ev = parse(json).unwrap().unwrap();
        assert_eq!(ev.kind(), "userquake");
        let EventBody::Userquake(u) = &ev.body else { panic!() };
        assert_eq!((u.count, u.started_at.as_str()), (12, "2026/09/29 07:33:29.873"));
        assert_eq!(
            u.areas,
            vec![
                UserquakeArea {
                    code: 270,
                    count: 8,
                    confidence: 0.85
                },
                UserquakeArea {
                    code: 275,
                    count: 4,
                    confidence: 0.3
                },
            ]
        );
        // 信頼度 0 (P2P地震情報でも非表示) は扱わない
        let hidden = r#"{"code":9611,"id":"u2","count":1,"confidence":0,"started_at":"s","updated_at":"u","area_confidences":{}}"#;
        assert!(parse(hidden).unwrap().is_none());
    }

    #[test]
    fn keeps_station_position_given_in_past_records() {
        let json = QUAKE.replace(
            r#"{"addr": "福島金山町川口", "isArea": false, "pref": "福島県", "scale": 10}"#,
            r#"{"addr": "旧観測点", "isArea": false, "pref": "福島県", "scale": 10, "station": {"lat": 37.1, "lon": 139.2, "area": "福島県会津"}}"#,
        );
        let ev = parse(&json).unwrap().unwrap();
        let EventBody::Quake(q) = &ev.body else { panic!() };
        assert_eq!(q.points[0].station, None);
        assert_eq!(
            q.points[2].station,
            Some(StationPos {
                lat: 37.1,
                lon: 139.2,
                area: "福島県会津".into()
            })
        );
    }

    #[test]
    fn parses_quake() {
        let ev = parse(QUAKE).unwrap().unwrap();
        assert_eq!(ev.id, "abc");
        let EventBody::Quake(q) = &ev.body else { panic!() };
        assert_eq!(q.info_type, QuakeInfoType::DetailScale);
        assert_eq!(q.max_scale, Scale::S3);
        assert_eq!(q.pref_max.len(), 2);
        assert_eq!(
            q.pref_max[0],
            PrefScale {
                pref: "福島県".into(),
                scale: Scale::S3
            }
        );
        let h = q.hypocenter.as_ref().unwrap();
        assert_eq!(h.depth_km, Some(10));
        assert_eq!(ev.title(), "【各地の震度に関する情報】福島県会津 最大震度3");
    }

    #[test]
    fn unknown_hypocenter_values_become_none() {
        let json = r#"{"code":551,"id":"x","issue":{"time":"t","type":"ScalePrompt"},
          "earthquake":{"time":"t","maxScale":45,"domesticTsunami":"Checking",
            "hypocenter":{"name":"","latitude":-200,"longitude":-200,"depth":-1,"magnitude":-1}},
          "points":[{"pref":"石川県","addr":"能登地方","isArea":true,"scale":45}]}"#;
        let ev = parse(json).unwrap().unwrap();
        let EventBody::Quake(q) = ev.body else { panic!() };
        assert!(q.hypocenter.is_none());
        assert_eq!(q.info_type, QuakeInfoType::ScalePrompt);
    }

    #[test]
    fn ignores_peer_messages() {
        assert!(parse(r#"{"code":555,"_id":"p","areas":[]}"#).unwrap().is_none());
    }

    #[test]
    fn parses_eew() {
        let json = r#"{"code":556,"_id":"e1","test":false,"cancelled":false,
          "issue":{"time":"2026/09/28 10:00:05","eventId":"20260928100000","serial":"3"},
          "earthquake":{"originTime":"2026/09/28 10:00:00","arrivalTime":"2026/09/28 10:00:02","condition":"",
            "hypocenter":{"name":"宮城県沖","reduceName":"宮城県","latitude":38.2,"longitude":142.0,"depth":40,"magnitude":6.8}},
          "areas":[
            {"pref":"宮城県","name":"宮城県北部","scaleFrom":55,"scaleTo":60,"kindCode":"11","arrivalTime":null},
            {"pref":"岩手県","name":"岩手県沿岸南部","scaleFrom":50,"scaleTo":99,"kindCode":"10","arrivalTime":"2026/09/28 10:00:20"}
          ]}"#;
        let ev = parse(json).unwrap().unwrap();
        let EventBody::Eew(e) = &ev.body else { panic!() };
        assert_eq!(e.max_scale, Scale::S6_UPPER);
        assert!(e.areas[0].arrived);
        assert_eq!(e.areas[1].scale_to, None);
        assert_eq!(e.pref_max[0].pref, "宮城県");
        assert_eq!(ev.title(), "【緊急地震速報(警報)】宮城県沖 第3報");
    }

    #[test]
    fn eew_area_prefecture_comes_from_the_area_name() {
        let json = r#"{"code": 556, "_id": "h", "issue": {"time": "2022/08/11 00:53:24", "eventId": "20220811005302", "serial": "1"},
          "areas": [{"pref": "北海道道北", "name": "上川地方北部", "scaleFrom": 45, "scaleTo": 45, "kindCode": "10"}]}"#;
        let EventBody::Eew(e) = parse(json).unwrap().unwrap().body else {
            panic!()
        };
        assert_eq!(e.areas[0].pref, "北海道");
        assert_eq!(e.pref_max[0].pref, "北海道");
    }

    #[test]
    fn parses_cancelled_eew_without_earthquake() {
        let json = r#"{"code":556,"_id":"e2","cancelled":true,"issue":{"time":"t","eventId":"x","serial":"4"}}"#;
        let ev = parse(json).unwrap().unwrap();
        assert!(ev.title().ends_with("取消"));
    }

    #[test]
    fn parses_tsunami() {
        let json = r#"{"code":552,"_id":"t1","cancelled":false,"issue":{"source":"気象庁","time":"2026/09/28 10:05:00","type":"Focus"},
          "areas":[
            {"grade":"Watch","immediate":false,"name":"青森県太平洋沿岸"},
            {"grade":"Warning","immediate":true,"name":"宮城県","maxHeight":{"description":"３ｍ","value":3}}
          ]}"#;
        let ev = parse(json).unwrap().unwrap();
        let EventBody::Tsunami(t) = &ev.body else { panic!() };
        assert_eq!(t.areas[0].grade, TsunamiGrade::Warning);
        assert_eq!(t.areas[0].max_height.as_deref(), Some("３ｍ"));
        assert_eq!(ev.title(), "【津波警報】2地域");
    }

    #[test]
    fn shifts_issued_and_origin_times_separately() {
        let mut ev = parse(QUAKE).unwrap().unwrap();
        let issued = ev.issued_at_ms().unwrap();
        let EventBody::Quake(q) = &ev.body else { panic!() };
        let origin = q.origin_time_ms.unwrap();
        ev.shift_times(1_000, 60_000);
        assert_eq!(ev.issued_at_ms(), Some(issued + 1_000));
        let EventBody::Quake(q) = &ev.body else { panic!() };
        assert_eq!(q.origin_time_ms, Some(origin + 60_000));
        assert_eq!(q.origin_time, "2026/09/28 16:25:00");
    }

    #[test]
    fn roundtrips_normalized_json() {
        let ev = parse(QUAKE).unwrap().unwrap();
        let s = serde_json::to_string(&ev).unwrap();
        assert!(s.contains(r#""kind":"quake""#));
        let back: Event = serde_json::from_str(&s).unwrap();
        assert_eq!(back, ev);
    }
}
