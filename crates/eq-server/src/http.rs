//! ブラウザ向け HTTP / WebSocket。
//!
//! - `GET /ws`         : 接続時に `hello` (サーバ時刻 + 直近イベント)、以後 `event` を push
//! - `GET /api/events` : 直近イベントの JSON (WebSocket を使えないクライアント向け)
//! - `GET /api/source` : 取得元の種類 (`p2pquake` | `replay`)
//! - `GET /api/archive`: 過去の情報 (jsonl の sink があるときだけ。archive.rs)
//! - `GET /healthz`   : 死活監視
//! - それ以外          : `static_dir` の静的ファイル

use std::sync::Arc;
use std::time::Duration;

use crate::quake::Event;
use axum::extract::ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::Semaphore;
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::client_ip::client_ip;
use crate::hub::{now_ms, Hub};
use crate::ws_guard::{IpPermit, WsGuard};

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage<'a> {
    /// 接続直後に 1 回。server_time_ms はブラウザの時計ずれ補正 (P波・S波の描画) に使う。
    Hello {
        server_time_ms: u64,
        events: Vec<&'a Event>,
    },
    Event {
        server_time_ms: u64,
        event: &'a Event,
    },
}

// media-src の blob: は、履歴の再生とデモの読み上げ (POST で受けた WAV を blob: の URL で鳴らす) のため
const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; \
                   media-src 'self' blob:; connect-src 'self' ws: wss:; frame-ancestors 'self'; base-uri 'none'; \
                   form-action 'none'";

/// ブラウザから届くメッセージの上限。ブラウザは閉じる・ping への応答くらいしか送らない
const MAX_CLIENT_MESSAGE: usize = 16 * 1024;

/// 同時につなげるブラウザの数。つなぎっぱなしで大量に開かれてもメモリを使い切らないように (e2 はメモリ 1GB)
const MAX_CLIENTS: usize = 500;
static CLIENTS: Semaphore = Semaphore::const_new(MAX_CLIENTS);

/// `GET /api/source` : 情報の取得元の種類 (`p2pquake` | `replay`)。配信が、記録の再生をテスト表示なしで流さないための確認に使う
pub fn source_router(kind: &'static str) -> Router {
    Router::new().route(
        "/api/source",
        get(move || async move { Json(serde_json::json!({ "type": kind })) }),
    )
}

pub fn router(hub: Arc<Hub>, static_dir: &std::path::Path, extra: Vec<Router>, guard: Arc<WsGuard>) -> Router {
    let ws = Router::new()
        .route("/ws", get(ws_handler))
        .with_state((hub.clone(), guard));
    let mut app = Router::new()
        .route("/api/events", get(events_handler))
        .route("/healthz", get(|| async { "ok" }))
        .with_state(hub)
        .merge(ws);
    for r in extra {
        app = app.merge(r);
    }
    if !static_dir.as_os_str().is_empty() {
        app = app.fallback_service(ServeDir::new(static_dir));
    }
    let header = |name, value| SetResponseHeaderLayer::if_not_present(name, HeaderValue::from_static(value));
    app
        // 更新後に古い app.js がブラウザに残らないよう、毎回更新を確認させる (変わっていなければ 304)
        .layer(header(header::CACHE_CONTROL, "no-cache"))
        // 表示する文字列は上流の情報なので、万一のスクリプト注入に備えて読み込み先を自分だけに絞る
        // (style は要素の style 属性と SVG に埋め込んだ <style> を使うので inline を許す)
        .layer(header(header::CONTENT_SECURITY_POLICY, CSP))
        .layer(header(header::X_CONTENT_TYPE_OPTIONS, "nosniff"))
        .layer(header(header::X_FRAME_OPTIONS, "SAMEORIGIN"))
        .layer(header(header::REFERRER_POLICY, "same-origin"))
        .layer(CompressionLayer::new())
}

async fn events_handler(State(hub): State<Arc<Hub>>) -> impl IntoResponse {
    let events = hub.recent();
    Json(events.iter().map(|e| e.as_ref()).cloned().collect::<Vec<Event>>())
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State((hub, guard)): State<(Arc<Hub>, Arc<WsGuard>)>,
    peer: Option<axum::Extension<ConnectInfo<std::net::SocketAddr>>>,
    headers: HeaderMap,
) -> Response {
    let text = |name| headers.get(name).and_then(|v| v.to_str().ok());
    // 他サイトのページから、閲覧者のブラウザを使って接続枠を消費させない
    if !guard.origin_ok(text(header::ORIGIN), text(header::HOST)) {
        tracing::warn!(origin = ?text(header::ORIGIN), "WebSocket origin rejected");
        return StatusCode::FORBIDDEN.into_response();
    }
    let peer_ip = peer.map_or(std::net::Ipv4Addr::LOCALHOST.into(), |c| (c.0).0.ip());
    let ip = client_ip(peer_ip, &headers, &guard.trusted_proxies);
    let ip_permit = match guard.acquire(ip) {
        Ok(p) => p,
        Err(why) => {
            tracing::warn!(%ip, ?why, "WebSocket client limited");
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        }
    };
    let Ok(permit) = CLIENTS.try_acquire() else {
        tracing::warn!("too many WebSocket clients");
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    ws.max_message_size(MAX_CLIENT_MESSAGE)
        .on_upgrade(move |socket| async move {
            client(socket, hub, guard, ip_permit).await;
            drop(permit);
        })
}

async fn client(mut socket: WebSocket, hub: Arc<Hub>, guard: Arc<WsGuard>, _ip_permit: IpPermit) {
    let limits = &guard.limits;
    // 先に購読してから履歴を取ることで、その間に届いたイベントを取りこぼさない
    // (重複はブラウザ側で id により捨てる)
    let mut rx = hub.subscribe();
    let recent = hub.recent();
    let hello = ServerMessage::Hello {
        server_time_ms: now_ms(),
        events: recent.iter().map(|e| e.as_ref()).collect(),
    };
    if send(&mut socket, &hello, limits.send_timeout).await.is_err() {
        return;
    }
    // プロキシ (Caddy など) にアイドル切断されないよう定期的に ping。
    // ブラウザは ping に自動で pong を返す。何も届かないまま pong_timeout を過ぎたら切る
    let mut ping = tokio::time::interval(limits.ping_interval);
    ping.tick().await;
    let mut last_rx = tokio::time::Instant::now();
    loop {
        tokio::select! {
            r = rx.recv() => match r {
                Ok(ev) => {
                    let msg = ServerMessage::Event { server_time_ms: now_ms(), event: &ev };
                    if send(&mut socket, &msg, limits.send_timeout).await.is_err() {
                        return;
                    }
                }
                // 遅いクライアントは切断し、再接続時の hello で追いついてもらう
                Err(RecvError::Lagged(_)) | Err(RecvError::Closed) => return,
            },
            _ = ping.tick() => {
                if last_rx.elapsed() > limits.pong_timeout {
                    tracing::debug!("WebSocket pong timeout");
                    return;
                }
                if send_msg(&mut socket, Message::Ping(Default::default()), limits.send_timeout).await.is_err() {
                    return;
                }
            }
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                _ => last_rx = tokio::time::Instant::now(),
            },
        }
    }
}

/// 送信に期限を付ける。詰まったクライアントは Err にして切らせる
async fn send_msg(socket: &mut WebSocket, msg: Message, limit: Duration) -> Result<(), ()> {
    match tokio::time::timeout(limit, socket.send(msg)).await {
        Ok(Ok(())) => Ok(()),
        _ => Err(()),
    }
}

async fn send(socket: &mut WebSocket, msg: &ServerMessage<'_>, limit: Duration) -> Result<(), ()> {
    let text = serde_json::to_string(msg).map_err(|_| ())?;
    send_msg(socket, Message::Text(Utf8Bytes::from(text)), limit).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_guard() -> Arc<WsGuard> {
        WsGuard::new(crate::ws_guard::WsLimits::new(8, 30), vec![], vec![])
    }

    async fn get(app: Router, path: &str) -> (axum::http::response::Parts, String) {
        let res = app
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let (parts, body) = res.into_parts();
        (
            parts,
            String::from_utf8(to_bytes(body, 1 << 20).await.unwrap().to_vec()).unwrap(),
        )
    }

    #[tokio::test]
    async fn serves_events_with_security_headers() {
        let hub = Hub::new(10);
        let (parts, body) = get(router(hub, "".as_ref(), vec![], test_guard()), "/api/events").await;
        assert_eq!(parts.status, 200);
        assert_eq!(body, "[]");
        let h = |n| {
            parts
                .headers
                .get(n)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string()
        };
        assert_eq!(h(header::CACHE_CONTROL), "no-cache");
        assert_eq!(h(header::X_CONTENT_TYPE_OPTIONS), "nosniff");
        assert!(h(header::CONTENT_SECURITY_POLICY).contains("script-src 'self'"));
        // 履歴の再生とデモの読み上げは POST で受けた WAV を blob: の URL で鳴らす (docs/tts.md S11)
        assert!(h(header::CONTENT_SECURITY_POLICY).contains("media-src 'self' blob:"));
    }

    #[tokio::test]
    async fn source_reports_its_kind() {
        let (parts, body) = get(source_router("replay"), "/api/source").await;
        assert_eq!(parts.status, 200);
        assert_eq!(body, r#"{"type":"replay"}"#);
    }

    #[tokio::test]
    async fn static_files_do_not_escape_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "ok").unwrap();
        let app = || router(Hub::new(10), dir.path(), vec![], test_guard());
        assert_eq!(get(app(), "/").await.1, "ok");
        let (parts, _) = get(app(), "/../Cargo.toml").await;
        assert_ne!(parts.status, 200);
    }

    // ---- 実ソケット (ループバックで axum::serve) ----

    use crate::ws_guard::WsLimits;
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::{self, http::HeaderValue};

    async fn serve(limits: WsLimits) -> std::net::SocketAddr {
        let trusted = vec!["127.0.0.1".parse().unwrap()];
        let guard = WsGuard::new(limits, trusted, vec![]);
        let app = router(Hub::new(10), "".as_ref(), vec![], guard);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .unwrap();
        });
        addr
    }

    type Client = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

    async fn connect(
        addr: std::net::SocketAddr,
        headers: &[(&'static str, &str)],
    ) -> Result<Client, Box<tungstenite::Error>> {
        let mut req = format!("ws://{addr}/ws").into_client_request().unwrap();
        for (k, v) in headers {
            req.headers_mut().insert(*k, HeaderValue::from_str(v).unwrap());
        }
        tokio_tungstenite::connect_async(req)
            .await
            .map(|(c, _)| c)
            .map_err(Box::new)
    }

    fn status(r: Result<Client, Box<tungstenite::Error>>) -> u16 {
        match r {
            Ok(_) => 101,
            Err(e) => match *e {
                tungstenite::Error::Http(res) => res.status().as_u16(),
                e => panic!("{e}"),
            },
        }
    }

    #[tokio::test]
    async fn ws_rejects_foreign_origin_and_allows_same_origin_or_none() {
        let addr = serve(WsLimits::new(8, 100)).await;
        assert_eq!(
            status(connect(addr, &[("origin", "https://unrelated.example")]).await),
            403
        );
        assert_eq!(
            status(connect(addr, &[("origin", &format!("http://{addr}"))]).await),
            101
        );
        assert_eq!(status(connect(addr, &[]).await), 101);
    }

    #[tokio::test]
    async fn ws_limits_connections_per_client_ip() {
        let addr = serve(WsLimits::new(1, 100)).await;
        let first = connect(addr, &[]).await.unwrap();
        assert_eq!(status(connect(addr, &[]).await), 429);
        // 信頼するプロキシ (127.0.0.1) 経由なので、X-Forwarded-For の別の接続元は別枠
        assert_eq!(status(connect(addr, &[("x-forwarded-for", "203.0.113.9")]).await), 101);
        // 切れたら枠が戻る
        drop(first);
        let mut ok = false;
        for _ in 0..50 {
            if connect(addr, &[]).await.is_ok() {
                ok = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(ok);
    }

    #[tokio::test]
    async fn ws_limits_connection_rate_per_client_ip() {
        let addr = serve(WsLimits::new(100, 2)).await;
        for _ in 0..2 {
            drop(connect(addr, &[]).await.unwrap());
        }
        assert_eq!(status(connect(addr, &[]).await), 429);
    }

    #[tokio::test]
    async fn ws_drops_a_client_that_never_answers_ping() {
        let mut limits = WsLimits::new(8, 100);
        limits.ping_interval = Duration::from_millis(50);
        limits.pong_timeout = Duration::from_millis(200);
        let addr = serve(limits).await;
        // 読まない (= pong を返さない) まま待ち、そのあとで読むと、サーバ側から切られている
        let mut c = connect(addr, &[]).await.unwrap();
        tokio::time::sleep(Duration::from_millis(600)).await;
        let ended = tokio::time::timeout(Duration::from_secs(2), async {
            while let Some(Ok(m)) = c.next().await {
                if m.is_close() {
                    break;
                }
            }
        })
        .await;
        assert!(ended.is_ok(), "server kept the connection");
    }
}
