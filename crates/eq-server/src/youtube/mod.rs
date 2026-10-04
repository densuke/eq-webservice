//! YouTube への動画のアップロード (docs/replay-video.md 5.3・6.8)。
//! - `auth`: `eq-server youtube-auth` (一度だけの同意。ループバック + PKCE)
//! - `uploader`: 1 本ずつ上げる係 (トークンの更新・1 日の本数・割り当て・間の空け方)
//! - `api`: Google との通信 (trait の後ろ。テストは偽物)
//!
//! 動画の中身 (タイトル・説明文) は、キューを知っている replay-worker 側で作る。

pub mod api;
pub mod auth;
pub mod limit;
mod loopback;
mod pkce;
pub mod token;
pub mod uploader;

/// 今の時刻 (epoch ミリ秒)
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
