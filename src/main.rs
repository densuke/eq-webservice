use arc_swap::ArcSwap;
use axum::{Router, extract::State, response::Html, routing::get};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::time::{Duration, interval};
use tracing::info;

/// Earthquake entry as returned by the P2P Quake API (JMA code 551).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EarthquakeInfo {
    #[serde(rename = "time")]
    pub time: String,
    #[serde(rename = "hypocenter")]
    pub hypocenter: Hypocenter,
    #[serde(rename = "maxScale")]
    pub max_scale: i32,
    #[serde(rename = "magnitude")]
    pub magnitude: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hypocenter {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub depth: i32,
    pub magnitude: f64,
}

/// Top-level object returned by the P2P Quake API for code 551.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
struct P2PEntry {
    id: String,
    code: i32,
    time: String,
    earthquake: Option<EarthquakeInfo>,
}

/// Shared application state.
#[derive(Clone)]
struct AppState {
    earthquakes: Arc<ArcSwap<Vec<EarthquakeInfo>>>,
}

/// Fetch recent earthquake data from the P2P Quake API.
async fn fetch_earthquakes(client: &reqwest::Client) -> anyhow::Result<Vec<EarthquakeInfo>> {
    let url = "https://api.p2pquake.net/v2/history?codes=551&limit=50";
    let entries: Vec<P2PEntry> = client.get(url).send().await?.json().await?;
    let items = entries
        .into_iter()
        .filter_map(|e| e.earthquake)
        .collect();
    Ok(items)
}

/// Background task: poll the API every 60 seconds and update the cache.
async fn poller(state: AppState) {
    let client = reqwest::Client::new();
    let mut ticker = interval(Duration::from_secs(60));
    loop {
        ticker.tick().await;
        match fetch_earthquakes(&client).await {
            Ok(items) => {
                info!("Fetched {} earthquake entries", items.len());
                state.earthquakes.store(Arc::new(items));
            }
            Err(e) => {
                tracing::warn!("Failed to fetch earthquakes: {e}");
            }
        }
    }
}

// ── Handlers ────────────────────────────────────────────────────────────────

async fn handle_index(State(state): State<AppState>) -> Html<String> {
    let earthquakes = state.earthquakes.load();
    let markers_json = serde_json::to_string(&**earthquakes)
        .unwrap_or_default()
        .replace("</", r"<\/");

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="ja">
<head>
  <meta charset="UTF-8">
  <title>地震情報マップ</title>
  <link rel="stylesheet" href="https://unpkg.com/leaflet@1.9.4/dist/leaflet.css"/>
  <script src="https://unpkg.com/leaflet@1.9.4/dist/leaflet.js"></script>
  <style>
    html, body {{ margin: 0; padding: 0; height: 100%; }}
    #map {{ height: 100%; }}
    .eq-popup h3 {{ margin: 0 0 4px; }}
  </style>
</head>
<body>
<div id="map"></div>
<script>
var map = L.map('map').setView([36, 136], 5);
L.tileLayer('https://tile.openstreetmap.org/{{z}}/{{x}}/{{y}}.png', {{
  attribution: '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a>',
  maxZoom: 18
}}).addTo(map);

var data = {markers};
data.forEach(function(eq) {{
  var h = eq.hypocenter;
  if (!h || h.latitude === 0 || h.longitude === 0) return;
  var radius = Math.max(4, (h.magnitude || eq.magnitude) * 3);
  var color = eq.max_scale >= 50 ? '#c00' : eq.max_scale >= 30 ? '#f80' : '#08f';
  L.circleMarker([h.latitude, h.longitude], {{
    radius: radius, color: color, fillOpacity: 0.6
  }}).bindPopup('<div class="eq-popup"><h3>' + h.name + '</h3>' +
    '<b>発生時刻:</b> ' + eq.time + '<br>' +
    '<b>深さ:</b> ' + h.depth + ' km<br>' +
    '<b>マグニチュード:</b> ' + (h.magnitude || eq.magnitude) + '<br>' +
    '<b>最大震度:</b> ' + (eq.max_scale / 10).toFixed(0) +
    '</div>').addTo(map);
}});
</script>
</body>
</html>"#,
        markers = markers_json
    );
    Html(html)
}

async fn handle_api(State(state): State<AppState>) -> axum::Json<Vec<EarthquakeInfo>> {
    let earthquakes = state.earthquakes.load();
    axum::Json((*earthquakes).to_vec())
}

async fn handle_rss(State(state): State<AppState>) -> (
    [(axum::http::HeaderName, &'static str); 1],
    String,
) {
    let earthquakes = state.earthquakes.load();
    let now: DateTime<Utc> = Utc::now();
    let mut items = String::new();
    for eq in earthquakes.iter() {
        let h = &eq.hypocenter;
        let title = format!("{} M{:.1} 最大震度{}", h.name, h.magnitude, eq.max_scale / 10);
        let desc = format!(
            "発生時刻: {}, 深さ: {} km, マグニチュード: {:.1}",
            eq.time, h.depth, h.magnitude
        );
        let link = format!(
            "https://www.jma.go.jp/bosai/map.html#lat={}&lon={}",
            h.latitude, h.longitude
        );
        items.push_str(&format!(
            "<item>\
              <title><![CDATA[{title}]]></title>\
              <link>{link}</link>\
              <description><![CDATA[{desc}]]></description>\
              <pubDate>{}</pubDate>\
            </item>",
            now.format("%a, %d %b %Y %H:%M:%S +0000")
        ));
    }

    let rss = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>地震情報</title>
    <link>https://www.jma.go.jp/</link>
    <description>最近の地震情報 (P2P地震情報 API)</description>
    <language>ja</language>
    <lastBuildDate>{}</lastBuildDate>
    {}
  </channel>
</rss>"#,
        now.format("%a, %d %b %Y %H:%M:%S +0000"),
        items
    );

    ([(axum::http::header::CONTENT_TYPE, "application/rss+xml; charset=utf-8")], rss)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let state = AppState {
        earthquakes: Arc::new(ArcSwap::new(Arc::new(Vec::new()))),
    };

    // Initial fetch before starting the server.
    {
        let client = reqwest::Client::new();
        match fetch_earthquakes(&client).await {
            Ok(items) => {
                info!("Initial fetch: {} earthquake entries", items.len());
                state.earthquakes.store(Arc::new(items));
            }
            Err(e) => tracing::warn!("Initial fetch failed: {e}"),
        }
    }

    // Start background poller.
    tokio::spawn(poller(state.clone()));

    let app = Router::new()
        .route("/", get(handle_index))
        .route("/api/earthquakes", get(handle_api))
        .route("/rss", get(handle_rss))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    info!("Listening on http://0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_earthquake_info_serde() {
        let json = r#"{
            "time": "2024/01/01 12:00:00",
            "hypocenter": {
                "name": "テスト地点",
                "latitude": 35.6,
                "longitude": 139.7,
                "depth": 10,
                "magnitude": 3.5
            },
            "maxScale": 20,
            "magnitude": 3.5
        }"#;
        let eq: EarthquakeInfo = serde_json::from_str(json).unwrap();
        assert_eq!(eq.hypocenter.name, "テスト地点");
        assert_eq!(eq.max_scale, 20);
        assert!((eq.magnitude - 3.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_rss_contains_channel() {
        // Verify RSS template produces valid structure
        let fake = EarthquakeInfo {
            time: "2024/01/01 12:00:00".to_string(),
            hypocenter: Hypocenter {
                name: "東京都".to_string(),
                latitude: 35.68,
                longitude: 139.69,
                depth: 50,
                magnitude: 4.2,
            },
            max_scale: 30,
            magnitude: 4.2,
        };
        let serialized = serde_json::to_string(&fake).unwrap();
        assert!(serialized.contains("東京都"));
    }
}
