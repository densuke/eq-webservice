//! ブラウザ向け HTTP / WebSocket。
//!
//! - `GET /ws`         : 接続時に `hello` (サーバ時刻 + 直近イベント)、以後 `event` を push
//! - `GET /api/events` : 直近イベントの JSON (WebSocket を使えないクライアント向け)
//! - `GET /healthz`    : 死活監視
//! - それ以外          : `static_dir` の静的ファイル

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, HeaderValue};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use eq_core::Event;
use serde::Serialize;
use tokio::sync::broadcast::error::RecvError;
use tower_http::compression::predicate::{DefaultPredicate, NotForContentType, Predicate};
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::hub::{now_ms, Hub};

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

const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; \
                   connect-src 'self' ws: wss:; frame-ancestors 'self'; base-uri 'none'; form-action 'none'";

/// ブラウザから届くメッセージの上限。ブラウザは閉じる・ping への応答くらいしか送らない
const MAX_CLIENT_MESSAGE: usize = 16 * 1024;

pub fn router(hub: Arc<Hub>, static_dir: &std::path::Path, extra: Vec<Router>) -> Router {
    let mut app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/api/events", get(events_handler))
        .route("/healthz", get(|| async { "ok" }))
        .with_state(hub);
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
        // 音声 (BGM) は既に圧縮されているので圧縮しない
        .layer(
            CompressionLayer::new().compress_when(DefaultPredicate::new().and(NotForContentType::const_new("audio/"))),
        )
}

async fn events_handler(State(hub): State<Arc<Hub>>) -> impl IntoResponse {
    let events = hub.recent();
    Json(events.iter().map(|e| e.as_ref()).cloned().collect::<Vec<Event>>())
}

async fn ws_handler(ws: WebSocketUpgrade, State(hub): State<Arc<Hub>>) -> impl IntoResponse {
    ws.max_message_size(MAX_CLIENT_MESSAGE)
        .on_upgrade(move |socket| client(socket, hub))
}

async fn client(mut socket: WebSocket, hub: Arc<Hub>) {
    // 先に購読してから履歴を取ることで、その間に届いたイベントを取りこぼさない
    // (重複はブラウザ側で id により捨てる)
    let mut rx = hub.subscribe();
    let recent = hub.recent();
    let hello = ServerMessage::Hello {
        server_time_ms: now_ms(),
        events: recent.iter().map(|e| e.as_ref()).collect(),
    };
    if send(&mut socket, &hello).await.is_err() {
        return;
    }
    // プロキシ (Caddy など) にアイドル切断されないよう定期的に ping
    let mut ping = tokio::time::interval(Duration::from_secs(30));
    ping.tick().await;
    loop {
        tokio::select! {
            r = rx.recv() => match r {
                Ok(ev) => {
                    let msg = ServerMessage::Event { server_time_ms: now_ms(), event: &ev };
                    if send(&mut socket, &msg).await.is_err() {
                        return;
                    }
                }
                // 遅いクライアントは切断し、再接続時の hello で追いついてもらう
                Err(RecvError::Lagged(_)) | Err(RecvError::Closed) => return,
            },
            _ = ping.tick() => {
                if socket.send(Message::Ping(Default::default())).await.is_err() {
                    return;
                }
            }
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                _ => {}
            },
        }
    }
}

async fn send(socket: &mut WebSocket, msg: &ServerMessage<'_>) -> Result<(), ()> {
    let text = serde_json::to_string(msg).map_err(|_| ())?;
    socket.send(Message::Text(Utf8Bytes::from(text))).await.map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower::ServiceExt;

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
        let (parts, body) = get(router(hub, "".as_ref(), vec![]), "/api/events").await;
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
    }

    #[tokio::test]
    async fn static_files_do_not_escape_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "ok").unwrap();
        let app = || router(Hub::new(10), dir.path(), vec![]);
        assert_eq!(get(app(), "/").await.1, "ok");
        let (parts, _) = get(app(), "/../Cargo.toml").await;
        assert_ne!(parts.status, 200);
    }
}
