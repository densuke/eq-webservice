//! 配信先プラグイン。
//!
//! プラグインを追加するには:
//! 1. `handle` (と必要なら `seed` / `routes`) を持つ型を作る (このディレクトリにモジュールを追加)
//! 2. `Sink` の列挙と、`build()` の match に `type` 名を追加する
//!
//! 各プラグインは独立した tokio タスクで動くため、遅いプラグイン (外部 HTTP など) が
//! 他のプラグインやブラウザへの配信を止めることはない。

mod discord;
mod jsonl;
mod rss;
mod webhook;

use std::sync::Arc;
use std::time::Duration;

use crate::quake::{Event, Scale};
use anyhow::Context;
use axum::Router;
use serde::Deserialize;
use tokio::sync::broadcast::error::RecvError;

use crate::hub::Hub;

pub enum Sink {
    Rss(rss::RssSink),
    Discord(discord::DiscordSink),
    Jsonl(jsonl::JsonlSink),
    Webhook(webhook::WebhookSink),
}

impl Sink {
    /// 1 件の処理の時間切れ。jsonl は自分でやり直しを持ち、途中で切ると二重に書くので、保険の長さだけにする
    fn timeout(&self) -> Duration {
        match self {
            Sink::Jsonl(_) => Duration::from_secs(600),
            _ => Duration::from_secs(30),
        }
    }

    /// 新しいイベントを受け取る。
    pub async fn handle(&self, ev: &Event) -> anyhow::Result<()> {
        match self {
            Sink::Rss(s) => s.handle(ev).await,
            Sink::Discord(s) => s.handle(ev).await,
            Sink::Jsonl(s) => s.handle(ev).await,
            Sink::Webhook(s) => s.handle(ev).await,
        }
    }

    /// 起動時に取り込んだ履歴。蓄積するもの (RSS) だけが使う
    pub async fn seed(&self, events: &[Arc<Event>]) -> anyhow::Result<()> {
        match self {
            Sink::Rss(s) => s.seed(events).await,
            _ => Ok(()),
        }
    }

    /// HTTP で公開するもの (RSS フィード) があればルーティングを返す。
    pub fn routes(&self) -> Option<Router> {
        match self {
            Sink::Rss(s) => Some(s.routes()),
            _ => None,
        }
    }
}

/// 全プラグイン共通の設定
#[derive(Debug, Deserialize)]
struct Common {
    r#type: String,
    /// ログ表示用の名前 (省略時は type)
    name: Option<String>,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(flatten)]
    filter: Filter,
}

fn default_true() -> bool {
    true
}

/// どのイベントをプラグインに渡すか
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Filter {
    /// 対象種別 ("quake" / "eew" / "eew_detection" / "tsunami" / "userquake")。空なら userquake 以外すべて。
    /// userquake (利用者の報告の集計で気象庁の発表ではない) は明示したときだけ渡す
    #[serde(default)]
    pub kinds: Vec<String>,
    /// 地震情報・EEW の最大震度がこれ未満なら渡さない ("3", "5弱", "5-" など)
    #[serde(default, deserialize_with = "de_scale")]
    pub min_scale: Option<Scale>,
}

impl Filter {
    pub fn accepts(&self, ev: &Event) -> bool {
        let listed = self.kinds.iter().any(|k| k == ev.kind());
        if (!self.kinds.is_empty() || ev.kind() == "userquake") && !listed {
            return false;
        }
        match (self.min_scale, ev.max_scale()) {
            (Some(min), Some(s)) => s >= min,
            _ => true,
        }
    }
}

fn de_scale<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Scale>, D::Error> {
    Option::<String>::deserialize(d)?
        .map(|s| Scale::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("unknown scale {s:?}"))))
        .transpose()
}

pub struct Loaded {
    pub name: String,
    pub filter: Filter,
    pub sink: Arc<Sink>,
}

/// 設定からプラグインを生成する。
pub fn build(table: &toml::Table) -> anyhow::Result<Option<Loaded>> {
    let common: Common = table.clone().try_into().context("sink config")?;
    if !common.enabled {
        return Ok(None);
    }
    let name = common.name.clone().unwrap_or_else(|| common.r#type.clone());
    // プラグイン固有の設定は、共通キーを取り除いた残り
    let mut own = table.clone();
    for k in ["type", "name", "enabled", "kinds", "min_scale"] {
        own.remove(k);
    }
    let sink = match common.r#type.as_str() {
        "rss" => Sink::Rss(rss::RssSink::new(own.try_into().context("rss config")?)?),
        "discord" => Sink::Discord(discord::DiscordSink::new(own.try_into().context("discord config")?)?),
        "jsonl" => Sink::Jsonl(jsonl::JsonlSink::new(own.try_into().context("jsonl config")?)),
        "webhook" => Sink::Webhook(webhook::WebhookSink::new(own.try_into().context("webhook config")?)?),
        other => anyhow::bail!("unknown sink type {other:?}"),
    };
    Ok(Some(Loaded {
        name,
        filter: common.filter,
        sink: Arc::new(sink),
    }))
}

/// 1 つのプラグインにイベントを流し続けるタスクを起動する。
pub fn spawn(loaded: Loaded, hub: &Hub) {
    let mut rx = hub.subscribe();
    tokio::spawn(async move {
        let Loaded { name, filter, sink } = loaded;
        loop {
            let ev = match rx.recv().await {
                Ok(ev) => ev,
                Err(RecvError::Lagged(n)) => {
                    tracing::warn!(sink = %name, skipped = n, "sink is too slow, events dropped");
                    continue;
                }
                Err(RecvError::Closed) => return,
            };
            if !filter.accepts(&ev) {
                continue;
            }
            match tokio::time::timeout(sink.timeout(), sink.handle(&ev)).await {
                Ok(Ok(())) => tracing::debug!(sink = %name, id = %ev.id, "delivered"),
                Ok(Err(e)) => tracing::warn!(sink = %name, "failed: {e:#}"),
                Err(_) => tracing::warn!(sink = %name, "timed out"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quake::p2pquake;

    fn quake(scale: i32) -> Event {
        let json = format!(
            r#"{{"code":551,"id":"q","issue":{{"time":"2026/09/28 10:00:00","type":"ScalePrompt"}},
               "earthquake":{{"time":"2026/09/28 10:00:00","maxScale":{scale},"domesticTsunami":"None"}},"points":[]}}"#
        );
        p2pquake::parse(&json).unwrap().unwrap()
    }

    #[test]
    fn filter_by_scale_and_kind() {
        let t: toml::Table = toml::from_str(
            r#"type = "x"
min_scale = "5弱"
kinds = ["quake"]"#,
        )
        .unwrap();
        let c: Common = t.try_into().unwrap();
        assert!(!c.filter.accepts(&quake(40)));
        assert!(c.filter.accepts(&quake(45)));

        let t: toml::Table = toml::from_str("type = \"x\"\nmin_scale = \"3\"").unwrap();
        let c: Common = t.try_into().unwrap();
        assert_eq!(c.filter.min_scale, Some(Scale::S3));
    }

    #[test]
    fn userquake_goes_to_sinks_only_when_listed() {
        let uq = p2pquake::parse(
            r#"{"code":9611,"id":"u","count":3,"confidence":0.97,"started_at":"s","updated_at":"u","area_confidences":{}}"#,
        )
        .unwrap()
        .unwrap();
        // 種別を指定しない (すべて) でも、利用者の報告の集計は渡さない
        assert!(!Filter::default().accepts(&uq));
        assert!(Filter::default().accepts(&quake(30)));
        let listed = Filter {
            kinds: vec!["userquake".into()],
            min_scale: None,
        };
        assert!(listed.accepts(&uq));
    }

    #[test]
    fn unknown_type_is_error() {
        let t: toml::Table = toml::from_str("type = \"nope\"").unwrap();
        assert!(build(&t).is_err());
    }
}
