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
}

pub type Shared = Arc<RwLock<Option<CityWeather>>>;

/// latest_time.txt ("2026-09-29T13:50:00+09:00") から、実況のファイル名の時刻 ("20260929135000") を作る
pub fn amedas_key(latest: &str) -> Option<String> {
    let digits: String = latest.trim().get(..19)?.chars().filter(char::is_ascii_digit).collect();
    (digits.len() == 14).then_some(digits)
}

/// 府県の天気予報から、最初の区域の今日の天気 (コード, 文) を取り出す
pub fn parse_forecast(v: &Value) -> Option<(String, String)> {
    let area = v.get(0)?.get("timeSeries")?.get(0)?.get("areas")?.get(0)?;
    let code = area.get("weatherCodes")?.get(0)?.as_str()?;
    let text = area.get("weathers")?.get(0)?.as_str()?;
    Some((code.to_string(), text.replace(['　', ' '], "")))
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
pub fn summarize(
    observed_at: &str,
    obs: &Value,
    table: &Value,
    forecasts: &BTreeMap<String, (String, String)>,
) -> CityWeather {
    let cities = CITIES
        .iter()
        .filter_map(|(name, office, station)| {
            let (lat, lon) = position(table.get(station)?)?;
            let (code, text) = forecasts.get(*office).cloned().unwrap_or_default();
            let o = obs.get(station);
            Some(City {
                name: name.to_string(),
                lat: round3(lat),
                lon: round3(lon),
                code,
                text,
                temp: o.and_then(|o| value(o, "temp")),
                precip1h: o.and_then(|o| value(o, "precipitation1h")),
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
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .user_agent(concat!("eq-webservice/", env!("CARGO_PKG_VERSION")))
            .build()
        {
            Ok(c) => c,
            Err(e) => return tracing::warn!("city weather: {e:#}"),
        };
        let mut table = Value::Null;
        let mut forecasts = BTreeMap::new();
        let mut forecast_at: Option<Instant> = None;
        let mut last_time = String::new();
        loop {
            if forecast_at.is_none_or(|t| t.elapsed() >= FORECAST_EVERY) {
                forecast_at = Some(Instant::now());
                for (_, office, _) in CITIES {
                    match get_json(&client, &format!("{BOSAI}/forecast/data/forecast/{office}.json")).await {
                        Ok(v) => match parse_forecast(&v) {
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

async fn get_json(client: &reqwest::Client, url: &str) -> anyhow::Result<Value> {
    Ok(client.get(url).send().await?.error_for_status()?.json().await?)
}

/// 新しい実況があれば読んでまとめる (地点表は初回だけ読む)
async fn refresh(
    client: &reqwest::Client,
    table: &mut Value,
    last_time: &str,
    forecasts: &BTreeMap<String, (String, String)>,
) -> anyhow::Result<Option<CityWeather>> {
    let latest = client
        .get(format!("{BOSAI}/amedas/data/latest_time.txt"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
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

    #[test]
    fn takes_todays_weather_of_the_first_area() {
        let v = json!([{"timeSeries": [{"areas": [
            {"area": {"name": "東京地方"}, "weatherCodes": ["203", "300"], "weathers": ["くもり　時々　雨", "雨"]},
            {"area": {"name": "伊豆諸島北部"}, "weatherCodes": ["100"], "weathers": ["晴れ"]}
        ]}]}]);
        assert_eq!(parse_forecast(&v), Some(("203".into(), "くもり時々雨".into())));
        assert_eq!(parse_forecast(&json!({"error": 1})), None);
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
        let forecasts = BTreeMap::from([("130000".to_string(), ("300".to_string(), "雨".to_string()))]);
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
            }]
        );
        assert_eq!(w.rain, vec![[35.692, 139.75, 1.5]]);
    }
}
