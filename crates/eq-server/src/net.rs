//! 外への HTTP (気象庁・P2P地震情報・通知先)。応答の大きさに上限を付け、壊れた上流から巨大なデータが届いても
//! メモリを使い切らないようにする (e2 はメモリ 1GB)。

use std::time::Duration;

use anyhow::Context;
use serde::de::DeserializeOwned;

/// 取得する応答の上限。いちばん大きいアメダスの実況でも 300KB ほど
pub const MAX_BODY: usize = 8 << 20;
/// 上流の WebSocket の 1 メッセージの上限 (地震情報は大きくても数十 KB)
pub const MAX_WS_MESSAGE: usize = 1 << 20;

fn builder(timeout: Duration) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(concat!("eq-webservice/", env!("CARGO_PKG_VERSION")))
}

pub fn client(timeout: Duration) -> anyhow::Result<reqwest::Client> {
    Ok(builder(timeout).build()?)
}

/// リダイレクトを追わないクライアント (独自ヘッダーに秘密を載せる送信用。reqwest は Authorization 以外は転送先へ付けたままにする)
pub fn client_no_redirect(timeout: Duration) -> anyhow::Result<reqwest::Client> {
    client_with_redirect(timeout, reqwest::redirect::Policy::none())
}

/// リダイレクトの方針を指定するクライアント
pub fn client_with_redirect(timeout: Duration, policy: reqwest::redirect::Policy) -> anyhow::Result<reqwest::Client> {
    Ok(builder(timeout).redirect(policy).build()?)
}

/// 送って本文を読む。MAX_BODY を超えたら読むのをやめてエラーにする
pub async fn body(req: reqwest::RequestBuilder) -> anyhow::Result<Vec<u8>> {
    let mut res = req.send().await?.error_for_status()?;
    let mut out = Vec::new();
    while let Some(chunk) = res.chunk().await? {
        anyhow::ensure!(
            out.len() + chunk.len() <= MAX_BODY,
            "response is larger than {MAX_BODY} bytes"
        );
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

pub async fn text(req: reqwest::RequestBuilder) -> anyhow::Result<String> {
    String::from_utf8(body(req).await?).context("response is not UTF-8")
}

pub async fn json<T: DeserializeOwned>(req: reqwest::RequestBuilder) -> anyhow::Result<T> {
    serde_json::from_slice(&body(req).await?).context("parsing JSON")
}

/// 上流の WebSocket につなぐ (1 メッセージの大きさに上限を付ける)
pub async fn connect_ws(
    url: &str,
) -> anyhow::Result<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>> {
    let cfg = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(MAX_WS_MESSAGE))
        .max_frame_size(Some(MAX_WS_MESSAGE));
    let (ws, _) = tokio_tungstenite::connect_async_with_config(url, Some(cfg), false)
        .await
        .context("connect")?;
    Ok(ws)
}
