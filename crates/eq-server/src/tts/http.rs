//! docs/tts.md S5 を参照
//! 音声アナウンスの HTTP API。`GET /api/tts/event/{id}` (公開) と `POST /api/tts` (要認証)。

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tokio::sync::Semaphore;

use super::cache::{Cache, TooLong, TtsError};
use super::google::Synth;
use super::limits::sanitize;
use super::priors::priors_of;
use super::{phrase, wav};
use crate::hub::Hub;
use crate::quake::model::{Event, EventBody};

const MAX_TEXT_CHARS: usize = 500;

struct AppState<S> {
    hub: Arc<Hub>,
    cache: Option<Arc<Cache<S>>>,
    token: Option<String>,
    /// POST /api/tts/announce の同時実行枠 (満杯なら待たせず 503)
    announce_slots: Arc<Semaphore>,
}

/// POST /api/tts/announce の同時実行数。1 本あたり最大で 50MB 弱のメモリを使うので、256MB の MemoryMax に収まる数
const MAX_ANNOUNCE_CONCURRENCY: usize = 3;

pub fn router<S: Synth>(hub: Arc<Hub>, cache: Option<Arc<Cache<S>>>, token: Option<String>) -> Router {
    router_with_slots(hub, cache, token, Arc::new(Semaphore::new(MAX_ANNOUNCE_CONCURRENCY)))
}

fn router_with_slots<S: Synth>(
    hub: Arc<Hub>,
    cache: Option<Arc<Cache<S>>>,
    token: Option<String>,
    announce_slots: Arc<Semaphore>,
) -> Router {
    Router::new()
        .route("/api/tts/event/{id}", get(event::<S>))
        .route("/api/tts", post(custom::<S>))
        .route(
            "/api/tts/announce",
            post(announce::<S>).layer(DefaultBodyLimit::max(1 << 20)),
        )
        .with_state(Arc::new(AppState {
            hub,
            cache,
            token,
            announce_slots,
        }))
}

/// `?rate=22050` でブラウザ向けの小さい WAV にする。無ければ 44.1kHz (配信の mixer はこちら)
#[derive(Deserialize)]
struct RateQuery {
    rate: Option<u32>,
}

impl RateQuery {
    /// 受け付ける周波数か (44.1kHz とその半分だけ)
    fn valid(&self) -> bool {
        matches!(self.rate, None | Some(wav::RATE) | Some(22_050))
    }

    fn apply(&self, bytes: Vec<u8>) -> Vec<u8> {
        match self.rate {
            Some(22_050) => wav::half_rate(&bytes).unwrap_or(bytes),
            _ => bytes,
        }
    }
}

fn wav_response(bytes: Vec<u8>, cache_control: Option<&'static str>) -> Response {
    let mut res = ([(header::CONTENT_TYPE, "audio/wav")], bytes).into_response();
    if let Some(cc) = cache_control {
        res.headers_mut().insert(header::CACHE_CONTROL, cc.parse().unwrap());
    }
    res
}

async fn event<S: Synth>(
    State(st): State<Arc<AppState<S>>>,
    Path(id): Path<String>,
    Query(q): Query<RateQuery>,
) -> Response {
    if !q.valid() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(cache) = st.cache.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(ev) = st.hub.get(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // 続報の差分読み上げ用に、知っている報すべてから先行する報を探す
    let mut events: Vec<Event> = st.hub.snapshot().iter().map(|e| (**e).clone()).collect();
    if !events.iter().any(|e| e.id == ev.id) {
        events.push((*ev).clone());
    }
    let priors = priors_of(&ev, &events);
    let segs = phrase::announce_segments(&ev, &priors);
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
        Ok(bytes) => wav_response(q.apply(bytes), Some("public, max-age=86400")),
        Err(TtsError::Budget) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
        Err(TtsError::Failed(e)) => {
            tracing::warn!("tts announce failed: {e:#}");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// 履歴の再生・デモ用。キャッシュ済みの部品だけで組み立てる (docs/tts.md S10)
async fn announce<S: Synth>(State(st): State<Arc<AppState<S>>>, Query(q): Query<RateQuery>, body: String) -> Response {
    if !q.valid() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(cache) = st.cache.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(req) = serde_json::from_str::<AnnounceReq>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if req.priors.len() > MAX_PRIORS {
        return StatusCode::BAD_REQUEST.into_response();
    }
    // 件数・長さの上限と重複の除去 (S-01)。超えていれば 422
    let AnnounceReq { event, priors } = req;
    let Some(event) = sanitize(event) else {
        return StatusCode::UNPROCESSABLE_ENTITY.into_response();
    };
    let Some(mut priors) = priors.into_iter().map(sanitize).collect::<Option<Vec<_>>>() else {
        return StatusCode::UNPROCESSABLE_ENTITY.into_response();
    };
    // 重い処理 (差分の計算と PCM の組み立て) の前に枠を取る。満杯なら待たせない。応答を返し終えるまで持つ
    let Ok(_slot) = st.announce_slots.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    // 位置で順序を決められるよう、対象を末尾に置く
    priors.push(event.clone());
    let p = priors_of(&event, &priors);
    let segs = phrase::announce_segments(&event, &p);
    if segs.is_empty() {
        return StatusCode::NOT_FOUND.into_response();
    }
    match cache.announce_cached(&segs).await {
        Ok(Some(bytes)) => wav_response(q.apply(bytes), Some("no-store")),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(TooLong) => StatusCode::PAYLOAD_TOO_LARGE.into_response(),
    }
}

/// 履歴の再生に添える先行報の上限
const MAX_PRIORS: usize = 300;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AnnounceReq {
    event: Event,
    #[serde(default)]
    priors: Vec<Event>,
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

    /// 文字数と同じ長さのサンプルを返し、頼まれた文を記録する偽の合成器
    #[derive(Clone, Default)]
    struct Fake {
        texts: Arc<std::sync::Mutex<Vec<String>>>,
        /// 指定すると、文字数でなくこの長さのサンプルを返す
        samples: Option<usize>,
    }
    impl Fake {
        fn texts(&self) -> Vec<String> {
            self.texts.lock().unwrap().clone()
        }
    }
    impl Synth for Fake {
        async fn synth(&self, text: &str, _voice: &str) -> anyhow::Result<Vec<i16>> {
            self.texts.lock().unwrap().push(text.to_string());
            Ok(vec![100i16; self.samples.unwrap_or(text.chars().count())])
        }
    }

    const TOKEN: &str = "secret-token";

    struct Env {
        _dir: tempfile::TempDir,
        hub: Arc<Hub>,
        cache: Arc<Cache<Fake>>,
        fake: Fake,
    }

    fn env(limit: usize) -> Env {
        env_with(limit, Fake::default())
    }

    fn env_with(limit: usize, fake: Fake) -> Env {
        let dir = tempfile::tempdir().unwrap();
        let budget = Budget::load(dir.path().join("usage.json"), limit);
        let cache = Arc::new(Cache::new(
            dir.path().to_path_buf(),
            "ja-JP-Neural2-B".into(),
            fake.clone(),
            budget,
        ));
        Env {
            _dir: dir,
            hub: Hub::new(10),
            cache,
            fake,
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
    async fn get_event_with_rate_22050_returns_half_size_wav() {
        // ブラウザは ?rate=22050 で小さい WAV を受ける (配信の mixer は付けずに 44.1kHz)
        let e = env(1_000_000);
        e.hub.publish(event("q1", quake_body(QuakeInfoType::Destination)));
        let (_, full) = get(app(&e), "/api/tts/event/q1").await;
        let (parts, half) = get(app(&e), "/api/tts/event/q1?rate=22050").await;
        assert_eq!(parts.status, StatusCode::OK);
        assert_eq!(parts.headers[header::CONTENT_TYPE], "audio/wav");
        assert_eq!(u32::from_le_bytes(half[24..28].try_into().unwrap()), 22_050);
        assert!(half.len() < full.len() / 2 + 64);
    }

    #[tokio::test]
    async fn get_event_with_unknown_rate_is_400() {
        let e = env(1_000_000);
        e.hub.publish(event("q1", quake_body(QuakeInfoType::Destination)));
        let (parts, _) = get(app(&e), "/api/tts/event/q1?rate=12345").await;
        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
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
    async fn get_credible_userquake_returns_wav_even_after_a_newer_evaluation() {
        use crate::quake::model::UserquakeArea;
        let uq = |conf: f64| {
            EventBody::Userquake(Userquake {
                started_at: "2026/09/29 10:00:00.000".into(),
                updated_at: "2026/09/29 10:00:10.000".into(),
                count: 5,
                confidence: conf,
                areas: vec![UserquakeArea {
                    code: 205,
                    count: 3,
                    confidence: 0.9,
                }],
            })
        };
        let e = env(1_000_000);
        e.hub.publish(event("u1", uq(0.97)));
        e.hub.publish(event("u2", uq(0.98)));
        // 同じ揺れの古い評価も id で引ける (直近履歴からは消えても公開済みの記録に残る)
        for id in ["u1", "u2"] {
            let (parts, body) = get(app(&e), &format!("/api/tts/event/{id}")).await;
            assert_wav(&parts, &body);
        }
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

    // ---- 続報 (GET) と POST /api/tts/announce ----

    const T0: i64 = 1_700_000_000_000;

    /// 発生時刻と県別最大震度を指定できる地震情報
    fn quake_with(t: QuakeInfoType, prefs: &[(&str, Scale)], max: Scale) -> EventBody {
        let EventBody::Quake(mut q) = quake_body(t) else {
            unreachable!()
        };
        q.origin_time_ms = Some(T0);
        q.max_scale = max;
        q.pref_max = prefs
            .iter()
            .map(|(p, s)| crate::quake::model::PrefScale {
                pref: (*p).into(),
                scale: *s,
            })
            .collect();
        EventBody::Quake(q)
    }

    fn prior_body() -> EventBody {
        quake_with(
            QuakeInfoType::ScalePrompt,
            &[("石川県", Scale::S6_UPPER)],
            Scale::S6_UPPER,
        )
    }

    fn followup_body() -> EventBody {
        quake_with(QuakeInfoType::ScalePrompt, &[("石川県", Scale::S7)], Scale::S7)
    }

    const FOLLOW: &str = "続報。";
    const ISHIKAWA7: &str = "石川県で震度7を観測しました。";

    async fn post_announce(app: Router, json: serde_json::Value) -> (axum::http::response::Parts, Vec<u8>) {
        let req = Request::post("/api/tts/announce")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json.to_string()))
            .unwrap();
        call(app, req).await
    }

    fn announce_json(ev: Event, priors: Vec<Event>) -> serde_json::Value {
        serde_json::json!({"event": ev, "priors": priors})
    }

    #[tokio::test]
    async fn get_followup_quake_reads_only_the_difference() {
        let e = env(1_000_000);
        e.hub.publish(event("p1", prior_body()));
        e.hub.publish(event("f1", followup_body()));
        let (parts, body) = get(app(&e), "/api/tts/event/f1").await;
        assert_wav(&parts, &body);
        let texts = e.fake.texts();
        assert!(texts.contains(&FOLLOW.to_string()), "{texts:?}");
        assert!(texts.contains(&ISHIKAWA7.to_string()), "{texts:?}");
        assert!(!texts.contains(&"地震情報。".to_string()), "{texts:?}");
        assert!(!texts.contains(&"震度速報。".to_string()), "{texts:?}");
    }

    #[tokio::test]
    async fn get_followup_identical_to_prior_is_404() {
        let e = env(1_000_000);
        e.hub.publish(event("p1", prior_body()));
        e.hub.publish(event("f1", prior_body()));
        let (parts, _) = get(app(&e), "/api/tts/event/f1").await;
        assert_eq!(parts.status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn announce_without_cached_segments_is_404_and_does_not_synth() {
        let e = env(1_000_000);
        let j = announce_json(event("f1", followup_body()), vec![event("p1", prior_body())]);
        let (parts, _) = post_announce(app(&e), j).await;
        assert_eq!(parts.status, StatusCode::NOT_FOUND);
        assert_eq!(e.fake.texts().len(), 0);
    }

    #[tokio::test]
    async fn announce_with_rate_22050_returns_22050_hz_wav() {
        let e = env(1_000_000);
        e.cache.segment(FOLLOW, None).await.unwrap();
        e.cache.segment(ISHIKAWA7, None).await.unwrap();
        let j = announce_json(event("f1", followup_body()), vec![event("p1", prior_body())]);
        let req = Request::post("/api/tts/announce?rate=22050")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(j.to_string()))
            .unwrap();
        let (parts, body) = call(app(&e), req).await;
        assert_eq!(parts.status, StatusCode::OK);
        assert_eq!(u32::from_le_bytes(body[24..28].try_into().unwrap()), 22_050);
    }

    #[tokio::test]
    async fn announce_with_cached_segments_returns_no_store_wav_without_synth() {
        let e = env(1_000_000);
        e.cache.segment(FOLLOW, None).await.unwrap();
        e.cache.segment(ISHIKAWA7, None).await.unwrap();
        let before = e.fake.texts().len();
        let j = announce_json(event("f1", followup_body()), vec![event("p1", prior_body())]);
        let (parts, body) = post_announce(app(&e), j).await;
        assert_wav(&parts, &body);
        assert_eq!(parts.headers[header::CACHE_CONTROL], "no-store");
        assert_eq!(e.fake.texts().len(), before);
    }

    #[tokio::test]
    async fn announce_appends_event_to_priors_when_received_at_is_zero() {
        let e = env(1_000_000);
        e.cache.segment(FOLLOW, None).await.unwrap();
        e.cache.segment(ISHIKAWA7, None).await.unwrap();
        // デモの報は received_at_ms がすべて 0。位置 (priors の後ろに event を足した並び) で続報を判定する
        let j = announce_json(event("f1", followup_body()), vec![event("p1", prior_body())]);
        let (_, body) = post_announce(app(&e), j).await;
        let want = FOLLOW.chars().count() + ISHIKAWA7.chars().count() + 6615;
        assert_eq!(wav::parse(&body).unwrap().len(), want);
    }

    #[tokio::test]
    async fn announce_with_301_priors_is_400() {
        let e = env(1_000_000);
        let priors: Vec<Event> = (0..301).map(|i| event(&format!("p{i}"), prior_body())).collect();
        let (parts, _) = post_announce(app(&e), announce_json(event("f1", followup_body()), priors)).await;
        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn announce_body_over_1mib_is_413() {
        let e = env(1_000_000);
        let EventBody::Quake(mut q) = followup_body() else {
            unreachable!()
        };
        q.comment = "x".repeat(3 << 19);
        let j = announce_json(event("f1", EventBody::Quake(q)), vec![]);
        let (parts, _) = post_announce(app(&e), j).await;
        assert_eq!(parts.status, StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn announce_without_cache_is_404() {
        let e = env(1_000_000);
        let app = router::<Fake>(e.hub.clone(), None, Some(TOKEN.into()));
        let j = announce_json(event("f1", followup_body()), vec![event("p1", prior_body())]);
        let (parts, _) = post_announce(app, j).await;
        assert_eq!(parts.status, StatusCode::NOT_FOUND);
    }

    // ---- S-01: POST /api/tts/announce の入力・出力・同時実行の上限 ----

    use crate::quake::model::{Tsunami, TsunamiArea, TsunamiGrade};

    fn tsunami_event(names: &[String], grade: TsunamiGrade) -> Event {
        event(
            "t1",
            EventBody::Tsunami(Tsunami {
                cancelled: false,
                issued_at: String::new(),
                areas: names
                    .iter()
                    .map(|n| TsunamiArea {
                        name: n.clone(),
                        grade,
                        immediate: false,
                        first_height: None,
                        max_height: None,
                    })
                    .collect(),
            }),
        )
    }

    fn area_names(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("予報区{i}")).collect()
    }

    /// 報が読む部品をすべてキャッシュに置く (合成器は fake)
    async fn warm(e: &Env, ev: &Event) {
        for seg in phrase::announce_segments(ev, &[]) {
            e.cache.segment(&seg, None).await.unwrap();
        }
    }

    #[tokio::test]
    async fn announce_tsunami_repeating_one_area_128_times_is_rejected() {
        // 監査 S-01 の再現: 同じ地域を 128 回含む津波警報 (約 963 倍に増幅していた)
        let e = env_with(
            1_000_000,
            Fake {
                samples: Some(44_100),
                ..Default::default()
            },
        );
        let ev = tsunami_event(&vec!["伊勢・三河湾".to_string(); 128], TsunamiGrade::MajorWarning);
        warm(
            &e,
            &tsunami_event(&["伊勢・三河湾".to_string()], TsunamiGrade::MajorWarning),
        )
        .await;
        let (parts, body) = post_announce(app(&e), announce_json(ev, vec![])).await;
        assert_eq!(parts.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(body.len() < 100);
    }

    #[tokio::test]
    async fn announce_duplicate_area_names_are_read_once() {
        let e = env(1_000_000);
        let one = tsunami_event(&["伊勢・三河湾".to_string()], TsunamiGrade::MajorWarning);
        warm(&e, &one).await;
        let many = tsunami_event(&vec!["伊勢・三河湾".to_string(); 100], TsunamiGrade::MajorWarning);
        let (_, a) = post_announce(app(&e), announce_json(one, vec![])).await;
        let (parts, b) = post_announce(app(&e), announce_json(many, vec![])).await;
        assert_eq!(parts.status, StatusCode::OK);
        assert_eq!(a, b);
    }

    #[tokio::test]
    async fn announce_legitimate_largest_major_tsunami_warning_passes() {
        // 大津波警報で全 66 予報区。1 部品 2 秒でも通る
        let e = env_with(
            10_000_000,
            Fake {
                samples: Some(2 * 44_100),
                ..Default::default()
            },
        );
        let ev = tsunami_event(&area_names(66), TsunamiGrade::MajorWarning);
        warm(&e, &ev).await;
        let (parts, body) = post_announce(app(&e), announce_json(ev, vec![])).await;
        assert_eq!(parts.status, StatusCode::OK);
        assert!(wav::parse(&body).unwrap().len() > 66 * 2 * 44_100);
    }

    #[tokio::test]
    async fn announce_output_over_the_sample_limit_is_413_before_reading() {
        let e = env_with(
            10_000_000,
            Fake {
                samples: Some(3 * 44_100),
                ..Default::default()
            },
        );
        // 上限の件数 (100 区) でも入力の検証は通るが、3 秒 x 100 = 300 秒は出力の上限 (180 秒) を超える
        let ev = tsunami_event(&area_names(100), TsunamiGrade::MajorWarning);
        warm(&e, &ev).await;
        let (parts, _) = post_announce(app(&e), announce_json(ev, vec![])).await;
        assert_eq!(parts.status, StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn announce_overlong_area_name_is_422() {
        let e = env(1_000_000);
        let ev = tsunami_event(&["あ".repeat(65)], TsunamiGrade::Warning);
        let (parts, _) = post_announce(app(&e), announce_json(ev, vec![])).await;
        assert_eq!(parts.status, StatusCode::UNPROCESSABLE_ENTITY);
        let ev = tsunami_event(&["あ".repeat(64)], TsunamiGrade::Warning);
        let (parts, _) = post_announce(app(&e), announce_json(ev, vec![])).await;
        assert_ne!(parts.status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn announce_prior_over_the_limits_is_422() {
        let e = env(1_000_000);
        let prior = tsunami_event(&vec!["x".to_string(); 101], TsunamiGrade::Warning);
        let (parts, _) = post_announce(app(&e), announce_json(followup_event(), vec![prior])).await;
        assert_eq!(parts.status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    fn followup_event() -> Event {
        event("f1", followup_body())
    }

    #[tokio::test]
    async fn announce_returns_503_when_all_slots_are_taken() {
        let e = env(1_000_000);
        e.cache.segment(FOLLOW, None).await.unwrap();
        e.cache.segment(ISHIKAWA7, None).await.unwrap();
        let slots = Arc::new(Semaphore::new(2));
        let app = router_with_slots(e.hub.clone(), Some(e.cache.clone()), Some(TOKEN.into()), slots.clone());
        let j = announce_json(followup_event(), vec![event("p1", prior_body())]);
        let held: Vec<_> = (0..2).map(|_| slots.clone().try_acquire_owned().unwrap()).collect();
        let (parts, _) = post_announce(app.clone(), j.clone()).await;
        assert_eq!(parts.status, StatusCode::SERVICE_UNAVAILABLE);
        drop(held);
        let (parts, body) = post_announce(app.clone(), j.clone()).await;
        assert_wav(&parts, &body);
        // 応答を返し終えたら枠も戻る
        assert_eq!(slots.available_permits(), 2);
    }
}
