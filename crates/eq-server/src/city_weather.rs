//! 平時に出す主要都市の天気。気象庁の防災情報サイト (bosai) の天気予報 (府県ごと) とアメダスの実況を定期的に取得し、
//! 都市ごとの天気・気温と、雨の降っている地点をまとめて `GET /api/weather` で返す。
//! bosai の JSON は仕様が公開されていないので、形が変わって読めなければ null を返すだけにする (地震の表示には影響させない)。
//! 出典: 気象庁 (https://www.jma.go.jp/bosai/)

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use serde_json::Value;

const BOSAI: &str = "https://www.jma.go.jp/bosai";
/// アメダスは 10 分ごとに更新される
const AMEDAS_EVERY: Duration = Duration::from_secs(600);
/// 天気予報は 1 日に数回なので 1 時間ごとに見る
/// 雨の地点として出す 1 時間降水量 (これ未満の弱い雨まで出すと点が多すぎる)
const RAIN_MIN_MM: f64 = 1.0;
const FORECAST_EVERY: Duration = Duration::from_secs(3600);

/// 主要都市: (名前, 天気予報の府県コード, アメダスの地点番号)。天気は府県の最初の一次細分区域 (その都市を含む地方) を使う
const CITIES: [(&str, &str, &str); 13] = [
    ("札幌", "016000", "14163"),
    ("仙台", "040000", "34392"),
    ("新潟", "150000", "54232"),
    ("東京", "130000", "44132"),
    ("千葉", "120000", "45212"),
    ("名古屋", "230000", "51106"),
    ("大阪", "270000", "62078"),
    ("神戸", "280000", "63518"),
    ("広島", "340000", "67437"),
    ("高知", "390000", "74182"),
    ("福岡", "400000", "82182"),
    ("鹿児島", "460100", "88317"),
    ("那覇", "471000", "91197"),
];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CityWeather {
    /// アメダスの観測時刻
    pub observed_at: String,
    pub cities: Vec<City>,
    /// 1 時間降水量が RAIN_MIN_MM 以上の地点 [緯度, 経度, mm]
    pub rain: Vec<[f64; 3]>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct City {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    /// 天気予報の天気コード (100 = 晴れ など)。予報が取れていなければ空
    pub code: String,
    /// "くもり時々雨" など
    pub text: String,
    /// 気温 (℃)
    pub temp: Option<f64>,
    /// 1 時間降水量 (mm)
    pub precip1h: Option<f64>,
    /// 明日の予報。取れていなければ null (足しただけなので、古いクライアントは無視できる)
    pub tomorrow: Option<Tomorrow>,
}

/// 明日の予報。気温・降水確率は、取れなかったものだけ null
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Tomorrow {
    pub code: String,
    pub text: String,
    /// 朝の最低気温 (℃)
    pub temp_min: Option<f64>,
    /// 日中の最高気温 (℃)
    pub temp_max: Option<f64>,
    /// 降水確率 (%)。明日の 6 時間ごとの値の最大
    pub pop: Option<u8>,
}

/// 府県の天気予報から取り出した、今日の天気と明日の予報
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Forecast {
    pub code: String,
    pub text: String,
    pub tomorrow: Option<Tomorrow>,
}

pub type Shared = Arc<RwLock<Option<CityWeather>>>;

/// latest_time.txt ("2026-09-29T13:50:00+09:00") から、実況のファイル名の時刻 ("20260929135000") を作る
pub fn amedas_key(latest: &str) -> Option<String> {
    let digits: String = latest.trim().get(..19)?.chars().filter(char::is_ascii_digit).collect();
    (digits.len() == 14).then_some(digits)
}

/// 明日の日付 ("2026-10-02"、JST)
pub fn tomorrow_date(now_ms: i64) -> String {
    crate::quake::jst::format(now_ms + 86_400_000)[..10].replace('/', "-")
}

/// 府県の天気予報から、最初の区域の今日の天気と、date (明日) の予報を取り出す。
///
/// 実物 (bosai/forecast/data/forecast/{office}.json) の [0] (短期予報) の timeSeries は、時刻 (timeDefines) の日付で見る:
/// - [0] 天気: 最初の区域の weatherCodes / weathers。明日は timeDefines が明日 00:00 の値
/// - [1] 降水確率: 6 時間ごと (06・12・18・00 時) の pops。明日の日付のものの最大を使う
/// - [2] 気温: 最初の地点の temps。明日 00:00 が朝の最低、明日 09:00 が日中の最高。
///   発表によっては片方が無いことがあるので、無ければ null にする。今日の 00:00・09:00 と混ざらないよう日付でも見る
pub fn parse_forecast(v: &Value, date: &str) -> Option<Forecast> {
    let series = v.get(0)?.get("timeSeries")?;
    let weather = series.get(0)?;
    let area = weather.get("areas")?.get(0)?;
    let codes = area.get("weatherCodes")?;
    let texts = area.get("weathers")?;
    let clean = |i: usize| Some(texts.get(i)?.as_str()?.replace(['　', ' '], ""));
    let tomorrow = times(weather).position(|t| at(t, date, "00")).and_then(|i| {
        Some(Tomorrow {
            code: codes.get(i)?.as_str()?.to_string(),
            text: clean(i)?,
            temp_min: temp_at(series.get(2), date, "00"),
            temp_max: temp_at(series.get(2), date, "09"),
            pop: max_pop(series.get(1), date),
        })
    });
    Some(Forecast {
        code: codes.get(0)?.as_str()?.to_string(),
        text: clean(0)?,
        tomorrow,
    })
}

fn times(series: &Value) -> impl Iterator<Item = &str> {
    series
        .get("timeDefines")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

/// timeDefines の時刻 ("2026-10-02T09:00:00+09:00") が、date の hour 時 ("09") か
fn at(t: &str, date: &str, hour: &str) -> bool {
    t.starts_with(date) && t.get(11..13) == Some(hour)
}

/// 気温の系列の、date の hour 時の値
fn temp_at(series: Option<&Value>, date: &str, hour: &str) -> Option<f64> {
    let series = series?;
    let i = times(series).position(|t| at(t, date, hour))?;
    series
        .get("areas")?
        .get(0)?
        .get("temps")?
        .get(i)?
        .as_str()?
        .parse()
        .ok()
}

/// 降水確率の系列の、date の日付のものの最大 (空文字 "" は取れていない)
fn max_pop(series: Option<&Value>, date: &str) -> Option<u8> {
    let series = series?;
    let pops = series.get("areas")?.get(0)?.get("pops")?.as_array()?;
    times(series)
        .zip(pops)
        .filter(|(t, _)| t.starts_with(date))
        .filter_map(|(_, p)| p.as_str()?.parse().ok())
        .max()
}

/// 実況の要素の値 ([値, 品質]。品質が 0 のものだけ使う)
fn value(obs: &Value, key: &str) -> Option<f64> {
    let pair = obs.get(key)?;
    (pair.get(1)?.as_i64()? == 0).then(|| pair.get(0)?.as_f64()).flatten()
}

/// 地点表の緯度・経度 ([度, 分])
fn position(station: &Value) -> Option<(f64, f64)> {
    let deg = |key: &str| {
        let a = station.get(key)?;
        Some(a.get(0)?.as_f64()? + a.get(1)?.as_f64()? / 60.0)
    };
    Some((deg("lat")?, deg("lon")?))
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// アメダスの実況・地点表と、府県ごとの天気予報をまとめる
pub fn summarize(observed_at: &str, obs: &Value, table: &Value, forecasts: &BTreeMap<String, Forecast>) -> CityWeather {
    let cities = CITIES
        .iter()
        .filter_map(|(name, office, station)| {
            let (lat, lon) = position(table.get(station)?)?;
            let f = forecasts.get(*office).cloned().unwrap_or_default();
            let o = obs.get(station);
            Some(City {
                name: name.to_string(),
                lat: round3(lat),
                lon: round3(lon),
                code: f.code,
                text: f.text,
                temp: o.and_then(|o| value(o, "temp")),
                precip1h: o.and_then(|o| value(o, "precipitation1h")),
                tomorrow: f.tomorrow,
            })
        })
        .collect();
    let rain = obs
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(station, o)| {
            let mm = value(o, "precipitation1h").filter(|mm| *mm >= RAIN_MIN_MM)?;
            let (lat, lon) = position(table.get(station)?)?;
            Some([round3(lat), round3(lon), mm])
        })
        .collect();
    CityWeather {
        observed_at: observed_at.trim().to_string(),
        cities,
        rain,
    }
}

/// 定期的に取得して shared を更新する
pub fn spawn(shared: Shared) {
    tokio::spawn(async move {
        let client = match crate::net::client(Duration::from_secs(60)) {
            Ok(c) => c,
            Err(e) => return tracing::warn!("city weather: {e:#}"),
        };
        let mut table = Value::Null;
        let mut forecasts = BTreeMap::new();
        let mut forecast_at: Option<Instant> = None;
        // 予報を取ったときの「明日」の日付 (日付が変わったら、明日の指す日が変わるので取り直す)
        let mut forecast_day = String::new();
        let mut last_time = String::new();
        loop {
            let tomorrow = tomorrow_date(now_ms());
            if forecast_at.is_none_or(|t| t.elapsed() >= FORECAST_EVERY) || tomorrow != forecast_day {
                forecast_at = Some(Instant::now());
                forecast_day = tomorrow.clone();
                for (_, office, _) in CITIES {
                    match get_json(&client, &format!("{BOSAI}/forecast/data/forecast/{office}.json")).await {
                        Ok(v) => match parse_forecast(&v, &tomorrow) {
                            Some(f) => {
                                forecasts.insert(office.to_string(), f);
                            }
                            None => tracing::warn!(office, "city weather: unexpected forecast"),
                        },
                        Err(e) => tracing::warn!(office, "city weather: {e:#}"),
                    }
                }
            }
            match refresh(&client, &mut table, &last_time, &forecasts).await {
                Ok(Some(w)) => {
                    tracing::info!(observed_at = %w.observed_at, rain = w.rain.len(), "city weather updated");
                    last_time = w.observed_at.clone();
                    *shared.write().unwrap() = Some(w);
                }
                Ok(None) => {}
                Err(e) => tracing::warn!("city weather: {e:#}"),
            }
            tokio::time::sleep(AMEDAS_EVERY).await;
        }
    });
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

async fn get_json(client: &reqwest::Client, url: &str) -> anyhow::Result<Value> {
    crate::net::json(client.get(url)).await
}

/// 新しい実況があれば読んでまとめる (地点表は初回だけ読む)
async fn refresh(
    client: &reqwest::Client,
    table: &mut Value,
    last_time: &str,
    forecasts: &BTreeMap<String, Forecast>,
) -> anyhow::Result<Option<CityWeather>> {
    let latest = crate::net::text(client.get(format!("{BOSAI}/amedas/data/latest_time.txt"))).await?;
    if latest.trim() == last_time {
        return Ok(None);
    }
    let key = amedas_key(&latest).ok_or_else(|| anyhow::anyhow!("unexpected latest_time: {latest:?}"))?;
    if table.is_null() {
        *table = get_json(client, &format!("{BOSAI}/amedas/const/amedastable.json")).await?;
    }
    let obs = get_json(client, &format!("{BOSAI}/amedas/data/map/{key}.json")).await?;
    Ok(Some(summarize(&latest, &obs, table, forecasts)))
}

/// `GET /api/weather` : 主要都市の天気と雨の地点 (まだ取得していなければ null)
pub fn router(shared: Shared) -> Router {
    Router::new().route(
        "/api/weather",
        get(move || async move { Json(shared.read().unwrap().clone()) }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn makes_the_amedas_file_key_from_the_latest_time() {
        assert_eq!(
            amedas_key("2026-09-29T13:50:00+09:00\n").as_deref(),
            Some("20260929135000")
        );
        assert_eq!(amedas_key("<html>"), None);
    }

    /// 2026-10-01 05 時発表の東京都の予報 (実物を小さくしたもの)
    fn tokyo() -> Value {
        json!([{"timeSeries": [
            {"timeDefines": ["2026-10-01T05:00:00+09:00", "2026-10-02T00:00:00+09:00"], "areas": [
                {"area": {"name": "東京地方"}, "weatherCodes": ["211", "200"], "weathers": ["くもり　昼過ぎ　から　晴れ", "くもり　所により　雨"]},
                {"area": {"name": "伊豆諸島北部"}, "weatherCodes": ["210", "200"], "weathers": ["くもり", "くもり"]}
            ]},
            {"timeDefines": ["2026-10-01T06:00:00+09:00", "2026-10-01T12:00:00+09:00", "2026-10-01T18:00:00+09:00",
                             "2026-10-02T00:00:00+09:00", "2026-10-02T06:00:00+09:00", "2026-10-02T12:00:00+09:00", "2026-10-02T18:00:00+09:00"],
             "areas": [{"area": {"name": "東京地方"}, "pops": ["20", "10", "10", "20", "30", "20", "20"]}]},
            {"timeDefines": ["2026-10-01T09:00:00+09:00", "2026-10-01T00:00:00+09:00", "2026-10-02T00:00:00+09:00", "2026-10-02T09:00:00+09:00"],
             "areas": [{"area": {"name": "東京"}, "temps": ["27", "27", "20", "22"]}]}
        ]}, {"timeSeries": []}])
    }

    #[test]
    fn takes_todays_weather_and_tomorrows_forecast_of_the_first_area() {
        let f = parse_forecast(&tokyo(), "2026-10-02").unwrap();
        assert_eq!((f.code.as_str(), f.text.as_str()), ("211", "くもり昼過ぎから晴れ"));
        // 今日の 00 時・09 時の気温 (27) と混ざらず、明日の朝の最低 20・日中の最高 22。降水確率は明日の 4 つの最大 (30)
        assert_eq!(
            f.tomorrow,
            Some(Tomorrow {
                code: "200".into(),
                text: "くもり所により雨".into(),
                temp_min: Some(20.0),
                temp_max: Some(22.0),
                pop: Some(30),
            })
        );
        assert_eq!(parse_forecast(&json!({"error": 1}), "2026-10-02"), None);
    }

    #[test]
    fn tomorrows_missing_values_are_null() {
        // 明日の最高気温・降水確率が無い
        let mut v = tokyo();
        v[0]["timeSeries"][2]["timeDefines"] = json!(["2026-10-01T00:00:00+09:00", "2026-10-02T00:00:00+09:00"]);
        v[0]["timeSeries"][2]["areas"][0]["temps"] = json!(["27", "20"]);
        v[0]["timeSeries"][1]["areas"][0]["pops"] = json!(["20", "10", "10", "", "", "", ""]);
        let t = parse_forecast(&v, "2026-10-02").unwrap().tomorrow.unwrap();
        assert_eq!((t.temp_min, t.temp_max, t.pop), (Some(20.0), None, None));
        // 気温・降水確率の系列そのものが無くても、天気は取れる
        v[0]["timeSeries"].as_array_mut().unwrap().truncate(1);
        let t = parse_forecast(&v, "2026-10-02").unwrap().tomorrow.unwrap();
        assert_eq!(
            (t.code.as_str(), t.temp_min, t.temp_max, t.pop),
            ("200", None, None, None)
        );
    }

    #[test]
    fn no_tomorrow_when_the_forecast_does_not_reach_it() {
        let f = parse_forecast(&tokyo(), "2026-10-03").unwrap();
        assert_eq!(f.tomorrow, None);
        assert_eq!(f.code, "211");
    }

    #[test]
    fn tomorrow_is_the_next_day_in_jst() {
        // 2026-10-01 23:30 JST・2026-10-02 00:10 JST・2026-10-01 00:00 JST
        assert_eq!(tomorrow_date(1_790_865_000_000), "2026-10-02");
        assert_eq!(tomorrow_date(1_790_867_400_000), "2026-10-03");
        assert_eq!(tomorrow_date(1_790_780_400_000), "2026-10-02");
    }

    #[test]
    fn combines_forecast_observation_and_rain_points() {
        let table = json!({
            "44132": {"lat": [35, 41.5], "lon": [139, 45.0], "kjName": "東京"},
            "11001": {"lat": [45, 31.2], "lon": [141, 56.1]},
            "11016": {"lat": [45, 24.9], "lon": [141, 40.7]}
        });
        let obs = json!({
            "44132": {"temp": [22.1, 0], "precipitation1h": [1.5, 0]},
            // 1mm 未満の弱い雨は出さない
            "11001": {"temp": [15.7, 0], "precipitation1h": [0.5, 0]},
            // 品質に問題のある値は使わない
            "11016": {"temp": [15.7, 0], "precipitation1h": [3.0, 5]}
        });
        let forecasts = BTreeMap::from([(
            "130000".to_string(),
            Forecast {
                code: "300".into(),
                text: "雨".into(),
                tomorrow: None,
            },
        )]);
        let w = summarize("2026-09-29T13:50:00+09:00\n", &obs, &table, &forecasts);
        assert_eq!(w.observed_at, "2026-09-29T13:50:00+09:00");
        // 地点表に無い都市は出さない
        assert_eq!(
            w.cities,
            vec![City {
                name: "東京".into(),
                lat: 35.692,
                lon: 139.75,
                code: "300".into(),
                text: "雨".into(),
                temp: Some(22.1),
                precip1h: Some(1.5),
                tomorrow: None,
            }]
        );
        assert_eq!(w.rain, vec![[35.692, 139.75, 1.5]]);
    }
}
