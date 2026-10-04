//! docs/tts.md S5 を参照
//! 音声アナウンスの HTTP API。`GET /api/tts/event/{id}` (公開) と `POST /api/tts` (要認証)。

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use super::cache::{Cache, TtsError};
use super::google::Synth;
use super::{phrase, wav};
use crate::hub::Hub;
use crate::quake::model::EventBody;

const MAX_TEXT_CHARS: usize = 500;

struct AppState<S> {
    hub: Arc<Hub>,
    cache: Option<Arc<Cache<S>>>,
    token: Option<String>,
}

pub fn router<S: Synth>(hub: Arc<Hub>, cache: Option<Arc<Cache<S>>>, token: Option<String>) -> Router {
    Router::new()
        .route("/api/tts/event/{id}", get(event::<S>))
        .route("/api/tts", post(custom::<S>))
        .with_state(Arc::new(AppState { hub, cache, token }))
}

fn wav_response(bytes: Vec<u8>, cache_control: Option<&'static str>) -> Response {
    let mut res = ([(header::CONTENT_TYPE, "audio/wav")], bytes).into_response();
    if let Some(cc) = cache_control {
        res.headers_mut().insert(header::CACHE_CONTROL, cc.parse().unwrap());
    }
    res
}

async fn event<S: Synth>(State(st): State<Arc<AppState<S>>>, Path(id): Path<String>) -> Response {
    let Some(cache) = st.cache.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(ev) = st.hub.get(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let segs = phrase::segments(&ev);
    if segs.is_empty() {
        return StatusCode::NOT_FOUND.into_response();
    }
    // EEW は数秒の遅れが致命的なので待ちを短くする
    let timeout = if matches!(ev.body, EventBody::Eew(_)) {
        Duration::from_millis(500)
    } else {
        Duration::from_secs(3)
    };
    match cache.announce(&segs, timeout).await {
        Ok(bytes) => wav_response(bytes, Some("public, max-age=86400")),
        Err(TtsError::Budget) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
        Err(TtsError::Failed(e)) => {
            tracing::warn!("tts announce failed: {e:#}");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CustomReq {
    text: String,
    voice: Option<String>,
}

async fn custom<S: Synth>(
    State(st): State<Arc<AppState<S>>>,
    headers: HeaderMap,
    body: Result<Json<CustomReq>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let (Some(token), Some(cache)) = (st.token.as_deref(), st.cache.as_ref()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    if !given.is_some_and(|g| ct_eq(g.as_bytes(), token.as_bytes())) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Ok(Json(req)) = body else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let chars = req.text.chars().count();
    if req.text.trim().is_empty() || chars > MAX_TEXT_CHARS {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if req
        .voice
        .as_deref()
        .is_some_and(|v| !valid_voice(v, cache.default_voice()))
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    match cache.segment(&req.text, req.voice.as_deref()).await {
        Ok(pcm) => wav_response(wav::encode(&pcm), None),
        Err(TtsError::Budget) => StatusCode::TOO_MANY_REQUESTS.into_response(),
        Err(TtsError::Failed(e)) => {
            tracing::warn!("tts synth failed: {e:#}");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// 定数時間の比較 (長さが違えば即 false)
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// 頼める声 (日本語の Neural2 と、設定の既定の声)。Studio・Chirp など単価の高い声を使わせない
const VOICES: [&str; 4] = [
    "ja-JP-Neural2-A",
    "ja-JP-Neural2-B",
    "ja-JP-Neural2-C",
    "ja-JP-Neural2-D",
];

fn valid_voice(v: &str, default: &str) -> bool {
    v == default || VOICES.contains(&v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quake::model::{Eew, Event, EventBody, Hypocenter, Quake, QuakeInfoType, Userquake};
    use crate::quake::scale::Scale;
    use crate::tts::budget::Budget;
    use crate::tts::wav;
    use axum::body::{to_bytes, Body};
    use axum::http::{header, Request, StatusCode};
    use tower::ServiceExt;

    /// 常に 10 サンプルを返す偽の合成器
    struct Fake;
    impl Synth for Fake {
        async fn synth(&self, _text: &str, _voice: &str) -> anyhow::Result<Vec<i16>> {
            Ok(vec![100i16; 10])
        }
    }

    const TOKEN: &str = "secret-token";

    struct Env {
        _dir: tempfile::TempDir,
        hub: Arc<Hub>,
        cache: Arc<Cache<Fake>>,
    }

    fn env(limit: usize) -> Env {
        let dir = tempfile::tempdir().unwrap();
        let budget = Budget::load(dir.path().join("usage.json"), limit);
        let cache = Arc::new(Cache::new(
            dir.path().to_path_buf(),
            "ja-JP-Neural2-B".into(),
            Fake,
            budget,
        ));
        Env {
            _dir: dir,
            hub: Hub::new(10),
            cache,
        }
    }

    fn app(e: &Env) -> Router {
        router(e.hub.clone(), Some(e.cache.clone()), Some(TOKEN.into()))
    }

    fn event(id: &str, body: EventBody) -> Event {
        Event {
            id: id.into(),
            source: "test".into(),
            received_at_ms: 0,
            body,
        }
    }

    fn quake_body(t: QuakeInfoType) -> EventBody {
        EventBody::Quake(Quake {
            info_type: t,
            origin_time: String::new(),
            origin_time_ms: None,
            issued_at: String::new(),
            hypocenter: Some(Hypocenter {
                name: "能登半島沖".into(),
                latitude: None,
                longitude: None,
                depth_km: None,
                magnitude: Some(5.0),
            }),
            max_scale: Scale::S4,
            domestic_tsunami: String::new(),
            points: vec![],
            pref_max: vec![],
            comment: String::new(),
        })
    }

    fn eew_body() -> EventBody {
        EventBody::Eew(Eew {
            event_id: "e".into(),
            serial: "1".into(),
            cancelled: false,
            test: false,
            warning: false,
            issued_at: String::new(),
            origin_time: None,
            origin_time_ms: None,
            hypocenter: Some(Hypocenter {
                name: "能登半島沖".into(),
                latitude: None,
                longitude: None,
                depth_km: None,
                magnitude: None,
            }),
            areas: vec![],
            pref_max: vec![],
            max_scale: Scale::S4,
        })
    }

    async fn call(app: Router, req: Request<Body>) -> (axum::http::response::Parts, Vec<u8>) {
        let (parts, body) = app.oneshot(req).await.unwrap().into_parts();
        (parts, to_bytes(body, 1 << 24).await.unwrap().to_vec())
    }

    async fn get(app: Router, path: &str) -> (axum::http::response::Parts, Vec<u8>) {
        call(app, Request::get(path).body(Body::empty()).unwrap()).await
    }

    async fn post(app: Router, auth: Option<&str>, json: serde_json::Value) -> (axum::http::response::Parts, Vec<u8>) {
        let mut b = Request::post("/api/tts").header(header::CONTENT_TYPE, "application/json");
        if let Some(a) = auth {
            b = b.header(header::AUTHORIZATION, a);
        }
        call(app, b.body(Body::from(json.to_string())).unwrap()).await
    }

    fn bearer() -> String {
        format!("Bearer {TOKEN}")
    }

    fn assert_wav(parts: &axum::http::response::Parts, body: &[u8]) {
        assert_eq!(parts.status, StatusCode::OK);
        assert_eq!(parts.headers[header::CONTENT_TYPE], "audio/wav");
        assert!(!wav::parse(body).unwrap().is_empty());
    }

    // ---- GET /api/tts/event/{id} ----

    #[tokio::test]
    async fn get_quake_event_returns_wav_with_cache_header() {
        let e = env(1_000_000);
        e.hub.publish(event("q1", quake_body(QuakeInfoType::Destination)));
        let (parts, body) = get(app(&e), "/api/tts/event/q1").await;
        assert_wav(&parts, &body);
        assert_eq!(parts.headers[header::CACHE_CONTROL], "public, max-age=86400");
    }

    #[tokio::test]
    async fn get_eew_event_returns_wav() {
        let e = env(1_000_000);
        e.hub.publish(event("w1", eew_body()));
        let (parts, body) = get(app(&e), "/api/tts/event/w1").await;
        assert_wav(&parts, &body);
    }

    #[tokio::test]
    async fn get_unknown_event_is_404() {
        let e = env(1_000_000);
        let (parts, _) = get(app(&e), "/api/tts/event/nope").await;
        assert_eq!(parts.status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_without_cache_is_404() {
        let e = env(1_000_000);
        e.hub.publish(event("q1", quake_body(QuakeInfoType::Destination)));
        let app = router::<Fake>(e.hub.clone(), None, Some(TOKEN.into()));
        let (parts, _) = get(app, "/api/tts/event/q1").await;
        assert_eq!(parts.status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_event_without_segments_is_404() {
        let e = env(1_000_000);
        e.hub.publish(event(
            "u1",
            EventBody::Userquake(Userquake {
                started_at: "s".into(),
                updated_at: "u".into(),
                count: 1,
                confidence: 0.5,
                areas: vec![],
            }),
        ));
        e.hub.publish(event("o1", quake_body(QuakeInfoType::Other)));
        for id in ["u1", "o1"] {
            let (parts, _) = get(app(&e), &format!("/api/tts/event/{id}")).await;
            assert_eq!(parts.status, StatusCode::NOT_FOUND, "{id}");
        }
    }

    #[tokio::test]
    async fn get_over_budget_is_503() {
        let e = env(0);
        e.hub.publish(event("q1", quake_body(QuakeInfoType::Destination)));
        let (parts, _) = get(app(&e), "/api/tts/event/q1").await;
        assert_eq!(parts.status, StatusCode::SERVICE_UNAVAILABLE);
    }

    // ---- POST /api/tts ----

    #[tokio::test]
    async fn post_without_token_configured_is_404() {
        let e = env(1_000_000);
        let app = router(e.hub.clone(), Some(e.cache.clone()), None);
        let (parts, _) = post(app, Some(&bearer()), serde_json::json!({"text": "こんにちは"})).await;
        assert_eq!(parts.status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn post_without_cache_is_404() {
        let e = env(1_000_000);
        let app = router::<Fake>(e.hub.clone(), None, Some(TOKEN.into()));
        let (parts, _) = post(app, Some(&bearer()), serde_json::json!({"text": "こんにちは"})).await;
        assert_eq!(parts.status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn post_without_authorization_is_401() {
        let e = env(1_000_000);
        let (parts, _) = post(app(&e), None, serde_json::json!({"text": "こんにちは"})).await;
        assert_eq!(parts.status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn post_with_wrong_bearer_is_401() {
        let e = env(1_000_000);
        let (parts, _) = post(app(&e), Some("Bearer wrong"), serde_json::json!({"text": "こんにちは"})).await;
        assert_eq!(parts.status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn post_with_correct_bearer_returns_wav() {
        let e = env(1_000_000);
        let (parts, body) = post(app(&e), Some(&bearer()), serde_json::json!({"text": "こんにちは"})).await;
        assert_wav(&parts, &body);
    }

    #[tokio::test]
    async fn post_empty_text_is_400() {
        let e = env(1_000_000);
        let (parts, _) = post(app(&e), Some(&bearer()), serde_json::json!({"text": ""})).await;
        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn post_text_length_limit_is_500_chars() {
        let e = env(1_000_000);
        let long = "あ".repeat(501);
        let (parts, _) = post(app(&e), Some(&bearer()), serde_json::json!({"text": long})).await;
        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
        let ok = "あ".repeat(500);
        let (parts, body) = post(app(&e), Some(&bearer()), serde_json::json!({"text": ok})).await;
        assert_wav(&parts, &body);
    }

    #[tokio::test]
    async fn post_voice_is_validated() {
        let e = env(1_000_000);
        let (parts, _) = post(
            app(&e),
            Some(&bearer()),
            serde_json::json!({"text": "あ", "voice": "en-US-x"}),
        )
        .await;
        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
        let (parts, body) = post(
            app(&e),
            Some(&bearer()),
            serde_json::json!({"text": "あ", "voice": "ja-JP-Neural2-C"}),
        )
        .await;
        assert_wav(&parts, &body);
        // ja-JP でも、単価の高い声系統は断る
        for v in ["ja-JP-Studio-B", "ja-JP-Chirp3-HD-Aoede", "ja-JP-Neural2-Z"] {
            let (parts, _) = post(app(&e), Some(&bearer()), serde_json::json!({"text": "あ", "voice": v})).await;
            assert_eq!(parts.status, StatusCode::BAD_REQUEST, "{v}");
        }
    }

    #[tokio::test]
    async fn post_over_budget_is_429() {
        let e = env(0);
        let (parts, _) = post(app(&e), Some(&bearer()), serde_json::json!({"text": "こんにちは"})).await;
        assert_eq!(parts.status, StatusCode::TOO_MANY_REQUESTS);
    }
}
