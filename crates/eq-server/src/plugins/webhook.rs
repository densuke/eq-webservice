//! 正規化済みイベントの JSON をそのまま任意の URL へ POST する。
//! Rust 以外で配信先を作りたい場合 (Slack 中継・スクリプトなど) の汎用の口。

use std::time::Duration;

use async_trait::async_trait;
use eq_core::Event;
use serde::Deserialize;

use super::Sink;

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
            client: reqwest::Client::builder().timeout(Duration::from_secs(10)).build()?,
        })
    }
}

#[async_trait]
impl Sink for WebhookSink {
    async fn handle(&self, ev: &Event) -> anyhow::Result<()> {
        let mut req = self.client.post(&self.cfg.url).json(ev);
        for (k, v) in &self.cfg.headers {
            req = req.header(k, v);
        }
        req.send().await?.error_for_status()?;
        Ok(())
    }
}
