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
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use eq_core::Event;
use serde::Serialize;
use tokio::sync::broadcast::error::RecvError;
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;

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
    app.layer(CompressionLayer::new())
}

async fn events_handler(State(hub): State<Arc<Hub>>) -> impl IntoResponse {
    let events = hub.recent();
    Json(events.iter().map(|e| e.as_ref()).cloned().collect::<Vec<Event>>())
}

async fn ws_handler(ws: WebSocketUpgrade, State(hub): State<Arc<Hub>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| client(socket, hub))
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
