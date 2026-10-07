//! 正規化済みイベントの JSON をそのまま任意の URL へ POST する。
//! Rust 以外で配信先を作りたい場合 (Slack 中継・スクリプトなど) の汎用の口。

use std::time::Duration;

use crate::quake::Event;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebhookConfig {
    url: String,
    /// 追加ヘッダ (認証トークンなど)
    #[serde(default)]
    headers: std::collections::BTreeMap<String, String>,
}

pub struct WebhookSink {
    cfg: WebhookConfig,
    client: reqwest::Client,
}

impl WebhookSink {
    pub fn new(cfg: WebhookConfig) -> anyhow::Result<Self> {
        Ok(WebhookSink {
            cfg,
            client: crate::net::client_no_redirect(Duration::from_secs(10))?,
        })
    }
}

impl WebhookSink {
    pub async fn handle(&self, ev: &Event) -> anyhow::Result<()> {
        let mut req = self.client.post(&self.cfg.url).json(ev);
        for (k, v) in &self.cfg.headers {
            req = req.header(k, v);
        }
        // URL にトークンを含めることがあるので、エラー (ログに出る) には含めない
        let res = req.send().await.map_err(reqwest::Error::without_url)?;
        // 独自ヘッダー (秘密) を別の送り先へ転送しないよう、リダイレクトは追わずに失敗として扱う
        anyhow::ensure!(
            !res.status().is_redirection(),
            "webhook answered with a redirect ({}); redirects are not followed",
            res.status()
        );
        res.error_for_status().map_err(reqwest::Error::without_url)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::post;
    use axum::Router;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    async fn serve(app: Router) -> std::net::SocketAddr {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, app).await });
        addr
    }

    #[tokio::test]
    async fn does_not_follow_redirects_so_custom_headers_stay_put() {
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        let target = serve(Router::new().route(
            "/",
            post(move || async move {
                h.fetch_add(1, Ordering::SeqCst);
            }),
        ))
        .await;
        let location = format!("http://{target}/");
        let origin = serve(Router::new().route(
            "/",
            post(move || async move { (axum::http::StatusCode::FOUND, [("location", location)]) }),
        ))
        .await;
        let ev: Event = serde_json::from_str(
            r#"{"id":"a","source":"wolfx","received_at_ms":1,"kind":"eew_detection","detection_type":"Full"}"#,
        )
        .unwrap();
        let sink = WebhookSink::new(WebhookConfig {
            url: format!("http://{origin}/"),
            headers: [("X-Api-Key".to_string(), "placeholder".to_string())].into(),
        })
        .unwrap();
        assert!(sink.handle(&ev).await.is_err());
        assert_eq!(hits.load(Ordering::SeqCst), 0);
    }
}
