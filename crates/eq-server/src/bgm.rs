//! 平時の BGM の配信先。音楽そのものは Icecast が配信し (送り出しは `eq-server bgm-send`)、画面はその URL を鳴らすだけ。
//! `GET /api/bgm` で画面に配信の URL と、再生中の曲名を取る Icecast の状態の URL を返す (設定が無ければ null)。

use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct BgmConfig {
    /// 画面が鳴らす配信の URL (同じサイトの中。例: "stream/bgm.mp3")。空なら BGM を使わない
    pub stream: String,
    /// 再生中の曲名を取る Icecast の状態 (status-json.xsl) の URL。空なら曲名は出さない
    pub status: String,
}

/// `GET /api/bgm`
pub fn router(cfg: &BgmConfig) -> Router {
    let body = (!cfg.stream.is_empty()).then(|| cfg.clone());
    Router::new().route("/api/bgm", get(move || async move { Json(body.clone()) }))
}
