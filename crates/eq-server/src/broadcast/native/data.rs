//! サーバから受け取るデータの形と、それを表示用にする小さな関数 (web/src/warnings.ts・weather.ts と同じ規則)。

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::quake::Event;

/// GET /api/warnings
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Warnings {
    /// 市町村等のコード -> 発表中の警報・注意報
    #[serde(default)]
    pub areas: BTreeMap<String, Vec<Kind>>,
    /// 市町村等のコード -> 名前
    #[serde(default)]
    pub names: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Kind {
    pub name: String,
}

/// GET /api/weather
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CityWeather {
    #[serde(default)]
    pub cities: Vec<City>,
    /// 1 時間降水量が 1mm 以上の地点 [緯度, 経度, mm]
    #[serde(default)]
    pub rain: Vec<[f64; 3]>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct City {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    #[serde(default)]
    pub code: String,
    pub temp: Option<f64>,
}

/// 警報・注意報の段階 (色分け)。低い順
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WarningLevel {
    Advisory,
    Warning,
    Danger,
    Emergency,
}

pub fn warning_level(name: &str) -> WarningLevel {
    if name.contains("特別警報") {
        WarningLevel::Emergency
    } else if name.contains("危険警報") {
        WarningLevel::Danger
    } else if name.contains("警報") {
        WarningLevel::Warning
    } else {
        WarningLevel::Advisory
    }
}

/// その区域で一番高い段階
pub fn top_level(kinds: &[Kind]) -> Option<WarningLevel> {
    kinds.iter().map(|k| warning_level(&k.name)).max()
}

/// 都道府県 (市町村等のコードの頭 2 桁が 01〜47 の番号)
const PREFS: [&str; 47] = [
    "北海道",
    "青森県",
    "岩手県",
    "宮城県",
    "秋田県",
    "山形県",
    "福島県",
    "茨城県",
    "栃木県",
    "群馬県",
    "埼玉県",
    "千葉県",
    "東京都",
    "神奈川県",
    "新潟県",
    "富山県",
    "石川県",
    "福井県",
    "山梨県",
    "長野県",
    "岐阜県",
    "静岡県",
    "愛知県",
    "三重県",
    "滋賀県",
    "京都府",
    "大阪府",
    "兵庫県",
    "奈良県",
    "和歌山県",
    "鳥取県",
    "島根県",
    "岡山県",
    "広島県",
    "山口県",
    "徳島県",
    "香川県",
    "愛媛県",
    "高知県",
    "福岡県",
    "佐賀県",
    "長崎県",
    "熊本県",
    "大分県",
    "宮崎県",
    "鹿児島県",
    "沖縄県",
];

pub fn pref_of(code: &str) -> &'static str {
    code.get(..2)
        .and_then(|n| n.parse::<usize>().ok())
        .and_then(|n| PREFS.get(n.checked_sub(1)?))
        .copied()
        .unwrap_or("")
}

/// 警報以上の帯の文 (web/src/warnings.ts の warningSummary)
#[derive(Debug, PartialEq)]
pub struct WarnSummary {
    pub top: WarningLevel,
    /// 種類ごとの文 (「レベル３大雨警報: 岡山県 岡山市・倉敷市」)。段階の高い順、同じなら名前順
    pub lines: Vec<String>,
}

/// 警報以上 (警報・危険警報・特別警報) を種類ごとの文にする。注意報は含めない。
/// 1 県 per_pref 市町村までで、残りは「ほかN」
pub fn warning_summary(w: &Warnings, per_pref: usize) -> Option<WarnSummary> {
    type Prefs<'a> = Vec<(&'static str, Vec<&'a str>)>;
    let mut by_kind: BTreeMap<&str, (WarningLevel, Prefs)> = BTreeMap::new();
    for (code, kinds) in &w.areas {
        for k in kinds {
            let level = warning_level(&k.name);
            if level == WarningLevel::Advisory {
                continue;
            }
            let (_, prefs) = by_kind.entry(&k.name).or_insert((level, Vec::new()));
            let pref = pref_of(code);
            let name = w.names.get(code).map_or(code.as_str(), String::as_str);
            match prefs.iter_mut().find(|(p, _)| *p == pref) {
                Some((_, names)) => names.push(name),
                None => prefs.push((pref, vec![name])),
            }
        }
    }
    let mut kinds: Vec<_> = by_kind.into_iter().collect();
    // BTreeMap は名前順なので、安定な並べ替えで段階の高い順だけ足す
    kinds.sort_by_key(|k| std::cmp::Reverse(k.1 .0));
    let top = kinds.first()?.1 .0;
    let lines = kinds
        .into_iter()
        .map(|(name, (_, prefs))| {
            let parts: Vec<String> = prefs
                .iter()
                .map(|(pref, ms)| {
                    let shown = ms[..ms.len().min(per_pref)].join("・");
                    match ms.len().checked_sub(per_pref).filter(|n| *n > 0) {
                        Some(n) => format!("{pref} {shown} ほか{n}"),
                        None => format!("{pref} {shown}"),
                    }
                })
                .collect();
            format!("{name}: {}", parts.join("、"))
        })
        .collect();
    Some(WarnSummary { top, lines })
}

/// 段階の塗りの色と不透明度 (web/src/map.css の .warn)
pub fn warning_fill(l: WarningLevel) -> ([u8; 3], f32) {
    match l {
        WarningLevel::Advisory => ([0xf2, 0xe7, 0x00], 0.5),
        WarningLevel::Warning => ([0xff, 0x28, 0x00], 0.65),
        WarningLevel::Danger => ([0xaa, 0x00, 0xff], 0.7),
        WarningLevel::Emergency => ([0x0c, 0x00, 0x0c], 0.85),
    }
}

/// 1 時間降水量の色 (気象庁の降水の配色)
pub fn rain_color(mm: f64) -> [u8; 3] {
    match mm {
        m if m >= 80.0 => [0xb4, 0x00, 0x68],
        m if m >= 50.0 => [0xff, 0x28, 0x00],
        m if m >= 30.0 => [0xff, 0x99, 0x00],
        m if m >= 20.0 => [0xfa, 0xf5, 0x00],
        m if m >= 10.0 => [0x00, 0x41, 0xff],
        m if m >= 5.0 => [0x21, 0x8c, 0xff],
        m if m >= 1.0 => [0xa0, 0xd2, 0xff],
        _ => [0xf2, 0xf2, 0xff],
    }
}

/// 天気の 1 文字 (天気コードの百の位。色付きの絵文字は描けないので文字で出す)
pub fn weather_char(code: &str) -> Option<char> {
    match code.chars().next()? {
        '1' => Some('晴'),
        '2' => Some('曇'),
        '3' => Some('雨'),
        '4' => Some('雪'),
        _ => None,
    }
}

/// 気温の表示 ("22°")。無ければ空
pub fn temp_label(t: Option<f64>) -> String {
    t.map_or_else(String::new, |t| format!("{}°", t.round() as i64))
}

/// 主要都市の札の向き。大阪と神戸、東京と千葉は近いので左右に分ける
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Side {
    Up,
    Down,
    Left,
    Right,
}

pub fn city_side(name: &str) -> Side {
    match name {
        "神戸" | "東京" => Side::Left,
        "大阪" | "千葉" => Side::Right,
        "高知" => Side::Down,
        _ => Side::Up,
    }
}

/// GET /ws のメッセージ (地震情報だけ使う)。読めない情報は捨てる
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Hello {
        server_time_ms: u64,
        #[serde(default)]
        events: Vec<serde_json::Value>,
    },
    Event {
        server_time_ms: u64,
        event: serde_json::Value,
    },
}

pub fn parse_events(values: Vec<serde_json::Value>) -> Vec<Event> {
    values
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect()
}

/// Icecast の status-json.xsl から、曲名を取り出す (source は 1 つなら object、複数なら配列)
pub fn bgm_title(status: &serde_json::Value) -> Option<String> {
    let src = &status["icestats"]["source"];
    let first = src.as_array().and_then(|a| a.first()).unwrap_or(src);
    first["title"].as_str().filter(|t| !t.is_empty()).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn warning_levels_follow_the_page() {
        assert_eq!(warning_level("レベル２大雨注意報"), WarningLevel::Advisory);
        assert_eq!(warning_level("レベル３大雨警報"), WarningLevel::Warning);
        assert_eq!(warning_level("レベル４危険警報"), WarningLevel::Danger);
        assert_eq!(warning_level("大雨特別警報"), WarningLevel::Emergency);
        let k = |n: &str| Kind { name: n.into() };
        assert_eq!(
            top_level(&[k("大雨注意報"), k("洪水警報")]),
            Some(WarningLevel::Warning)
        );
        assert_eq!(top_level(&[]), None);
    }

    #[test]
    fn warnings_and_above_are_summarized_like_the_page() {
        let k = |n: &str| vec![Kind { name: n.into() }];
        let mut w = Warnings::default();
        for (code, kinds, name) in [
            ("3310000", vec!["レベル３大雨警報", "雷注意報"], "岡山市"),
            ("3320200", vec!["レベル３大雨警報"], "倉敷市"),
            ("3420700", vec!["レベル３大雨警報"], "福山市"),
            ("3420200", vec!["レベル５大雨特別警報"], "呉市"),
            ("0110000", vec!["強風注意報"], "札幌市"),
        ] {
            w.areas.insert(code.into(), kinds.into_iter().flat_map(k).collect());
            w.names.insert(code.into(), name.into());
        }
        assert_eq!(pref_of("3310000"), "岡山県");
        assert_eq!(pref_of("9910000"), "");
        let s = warning_summary(&w, 5).unwrap();
        assert_eq!(s.top, WarningLevel::Emergency);
        assert_eq!(
            s.lines,
            [
                "レベル５大雨特別警報: 広島県 呉市",
                "レベル３大雨警報: 岡山県 岡山市・倉敷市、広島県 福山市"
            ]
        );
        assert_eq!(
            warning_summary(&w, 1).unwrap().lines[1],
            "レベル３大雨警報: 岡山県 岡山市 ほか1、広島県 福山市"
        );
        // 注意報だけなら出さない
        let mut adv = Warnings::default();
        adv.areas.insert("0110000".into(), k("強風注意報"));
        assert_eq!(warning_summary(&adv, 5), None);
    }

    #[test]
    fn weather_labels() {
        assert_eq!(weather_char("100"), Some('晴'));
        assert_eq!(weather_char("302"), Some('雨'));
        assert_eq!(weather_char(""), None);
        assert_eq!(temp_label(Some(23.6)), "24°");
        assert_eq!(temp_label(None), "");
        assert_eq!(rain_color(30.0), [0xff, 0x99, 0x00]);
        assert_eq!(rain_color(0.5), [0xf2, 0xf2, 0xff]);
        assert_eq!(city_side("大阪"), Side::Right);
    }

    #[test]
    fn parses_the_server_messages_and_skips_unreadable_events() {
        let m: ServerMessage = serde_json::from_value(json!({
            "type": "hello", "server_time_ms": 5, "events": [{"broken": true}]
        }))
        .unwrap();
        let ServerMessage::Hello { server_time_ms, events } = m else {
            panic!()
        };
        assert_eq!(server_time_ms, 5);
        assert!(parse_events(events).is_empty());
    }

    #[test]
    fn reads_the_bgm_title_from_icecast() {
        let one = json!({"icestats": {"source": {"title": "曲"}}});
        let many = json!({"icestats": {"source": [{"title": "曲2"}, {"title": "x"}]}});
        assert_eq!(bgm_title(&one).as_deref(), Some("曲"));
        assert_eq!(bgm_title(&many).as_deref(), Some("曲2"));
        assert_eq!(bgm_title(&json!({"icestats": {}})), None);
    }
}
