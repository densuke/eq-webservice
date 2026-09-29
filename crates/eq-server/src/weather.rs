//! 気象警報・注意報。気象庁防災情報XML (PULL 型) の「気象警報・注意報（Ｒ０６）（集約通報）」(VPWS50。全国分を
//! 10 分ごとに発表) を定期的に取得し、市町村等ごとに発表中の警報・注意報を保持して `GET /api/warnings` で返す。
//! 出典: 気象庁防災情報XML (https://xml.kishou.go.jp/)

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::Context;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct WeatherConfig {
    pub enabled: bool,
    /// 防災情報XML の定時の配信 (Atom)
    pub feed_url: String,
    /// 取得の間隔 (秒)。集約通報は 10 分ごと
    pub interval_sec: u64,
}

impl Default for WeatherConfig {
    fn default() -> Self {
        WeatherConfig {
            enabled: true,
            feed_url: "https://www.data.jma.go.jp/developer/xml/feed/regular.xml".into(),
            interval_sec: 300,
        }
    }
}

/// 市町村等ごとの発表中の警報・注意報
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Warnings {
    /// 集約通報の発表時刻
    pub reported_at: String,
    /// 市町村等のコード (7 桁) -> 発表中の種類。発表の無い区域は含めない
    pub areas: BTreeMap<String, Vec<Kind>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Kind {
    /// 気象庁の種別コード
    pub code: String,
    /// "レベル２大雨注意報" など
    pub name: String,
}

pub type Shared = Arc<RwLock<Option<Warnings>>>;

const FEED_NAMESPACE: &str = "http://www.w3.org/2005/Atom";
const MUNICIPAL: &str = "気象警報・注意報（市町村等）";

/// 配信 (Atom) から最新の集約通報の URL を探す
pub fn latest_report_url(feed: &str) -> anyhow::Result<Option<String>> {
    let doc = roxmltree::Document::parse(feed).context("parsing feed")?;
    let entries = doc.descendants().filter(|n| n.has_tag_name((FEED_NAMESPACE, "entry")));
    let mut found: Option<(String, String)> = None;
    for e in entries {
        let text = |tag: &str| {
            e.children()
                .find(|c| c.has_tag_name((FEED_NAMESPACE, tag)))
                .and_then(|c| c.text())
                .unwrap_or("")
        };
        let href = e
            .children()
            .find(|c| c.has_tag_name((FEED_NAMESPACE, "link")))
            .and_then(|c| c.attribute("href"))
            .unwrap_or("");
        if !href.contains("_VPWS50_") {
            continue;
        }
        let updated = text("updated").to_string();
        if found.as_ref().is_none_or(|(u, _)| updated > *u) {
            found = Some((updated, href.to_string()));
        }
    }
    Ok(found.map(|(_, h)| h))
}

/// 集約通報から、市町村等ごとの発表中の警報・注意報を取り出す (解除・発表なしは除く)
pub fn parse_report(xml: &str) -> anyhow::Result<Warnings> {
    let doc = roxmltree::Document::parse(xml).context("parsing report")?;
    let child_text = |n: roxmltree::Node, tag: &str| {
        n.children()
            .find(|c| c.tag_name().name() == tag)
            .and_then(|c| c.text())
            .unwrap_or("")
            .to_string()
    };
    let reported_at = doc
        .descendants()
        .find(|n| n.tag_name().name() == "ReportDateTime")
        .and_then(|n| n.text())
        .unwrap_or("")
        .to_string();
    let mut areas: BTreeMap<String, Vec<Kind>> = BTreeMap::new();
    let section = doc
        .descendants()
        .filter(|n| n.tag_name().name() == "Warning" && n.attribute("type") == Some(MUNICIPAL));
    for item in section.flat_map(|w| w.children().filter(|c| c.tag_name().name() == "Item")) {
        let Some(area) = item.children().find(|c| c.tag_name().name() == "Area") else {
            continue;
        };
        let kinds: Vec<Kind> = item
            .children()
            .filter(|c| c.tag_name().name() == "Kind")
            .filter(|k| !matches!(child_text(*k, "Status").as_str(), "解除" | "発表警報・注意報はなし"))
            .map(|k| Kind {
                code: child_text(k, "Code"),
                name: child_text(k, "Name"),
            })
            .filter(|k| !k.name.is_empty())
            .collect();
        if !kinds.is_empty() {
            areas.entry(child_text(area, "Code")).or_default().extend(kinds);
        }
    }
    Ok(Warnings { reported_at, areas })
}

/// 定期的に取得して shared を更新する
pub fn spawn(cfg: WeatherConfig, shared: Shared) {
    tokio::spawn(async move {
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .user_agent(concat!("eq-webservice/", env!("CARGO_PKG_VERSION")))
            .build()
        {
            Ok(c) => c,
            Err(e) => return tracing::warn!("weather: {e:#}"),
        };
        let mut last_url = String::new();
        loop {
            match refresh(&client, &cfg.feed_url, &last_url).await {
                Ok(Some((url, w))) => {
                    tracing::info!(areas = w.areas.len(), reported_at = %w.reported_at, "weather warnings updated");
                    *shared.write().unwrap() = Some(w);
                    last_url = url;
                }
                Ok(None) => {}
                Err(e) => tracing::warn!("weather: {e:#}"),
            }
            tokio::time::sleep(Duration::from_secs(cfg.interval_sec.max(60))).await;
        }
    });
}

async fn refresh(
    client: &reqwest::Client,
    feed_url: &str,
    last_url: &str,
) -> anyhow::Result<Option<(String, Warnings)>> {
    let feed = client.get(feed_url).send().await?.error_for_status()?.text().await?;
    let Some(url) = latest_report_url(&feed)? else {
        return Ok(None);
    };
    if url == last_url {
        return Ok(None);
    }
    let xml = client.get(&url).send().await?.error_for_status()?.text().await?;
    Ok(Some((url, parse_report(&xml)?)))
}

/// `GET /api/warnings` : 発表中の警報・注意報 (まだ取得していなければ null)
pub fn router(shared: Shared) -> Router {
    Router::new().route(
        "/api/warnings",
        get(move || async move { Json(shared.read().unwrap().clone()) }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_newest_aggregated_report_in_the_feed() {
        let feed = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <entry><title>府県天気概況</title><updated>2026-09-29T01:39:53Z</updated>
    <link type="application/xml" href="https://example/20260929013953_0_VPFG50_230000.xml"/></entry>
  <entry><title>気象警報・注意報（Ｒ０６）（集約通報）</title><updated>2026-09-29T01:20:56Z</updated>
    <link type="application/xml" href="https://example/20260929012059_0_VPWS50_010000.xml"/></entry>
  <entry><title>気象警報・注意報（Ｒ０６）（集約通報）</title><updated>2026-09-29T01:30:57Z</updated>
    <link type="application/xml" href="https://example/20260929013059_0_VPWS50_010000.xml"/></entry>
</feed>"#;
        assert_eq!(
            latest_report_url(feed).unwrap().as_deref(),
            Some("https://example/20260929013059_0_VPWS50_010000.xml")
        );
        assert_eq!(
            latest_report_url(r#"<feed xmlns="http://www.w3.org/2005/Atom"/>"#).unwrap(),
            None
        );
    }

    #[test]
    fn keeps_only_warnings_in_effect_per_municipality() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Report xmlns="http://xml.kishou.go.jp/jmaxml1/">
<Head xmlns="http://xml.kishou.go.jp/jmaxml1/informationBasis1/"><ReportDateTime>2026-09-29T10:30:00+09:00</ReportDateTime></Head>
<Body xmlns="http://xml.kishou.go.jp/jmaxml1/body/meteorology1/">
<Warning type="気象警報・注意報（府県予報区等）">
  <Item><Kind><Name>強風注意報</Name><Code>15</Code><Status>継続</Status></Kind><Area><Name>宗谷地方</Name><Code>011000</Code></Area></Item>
</Warning>
<Warning type="気象警報・注意報（市町村等）">
  <Item>
    <Kind><Name>雷注意報</Name><Code>14</Code><Status>解除</Status></Kind>
    <Kind><Name>強風注意報</Name><Code>15</Code><Status>解除</Status></Kind>
    <Area><Name>札幌市</Name><Code>0110000</Code></Area>
  </Item>
  <Item>
    <Kind><Name>レベル３大雨警報</Name><Code>03</Code><Status>発表</Status></Kind>
    <Kind><Name>強風注意報</Name><Code>15</Code><Status>継続</Status></Kind>
    <Area><Name>稚内市</Name><Code>0121400</Code></Area>
  </Item>
  <Item>
    <Kind><Name>発表警報・注意報はなし</Name><Status>発表警報・注意報はなし</Status></Kind>
    <Area><Name>旭川市</Name><Code>0120400</Code></Area>
  </Item>
</Warning>
</Body>
</Report>"#;
        let w = parse_report(xml).unwrap();
        assert_eq!(w.reported_at, "2026-09-29T10:30:00+09:00");
        // 市町村等の欄だけを見る。解除・発表なしは含めない
        assert_eq!(w.areas.keys().collect::<Vec<_>>(), vec!["0121400"]);
        assert_eq!(
            w.areas["0121400"],
            vec![
                Kind {
                    code: "03".into(),
                    name: "レベル３大雨警報".into()
                },
                Kind {
                    code: "15".into(),
                    name: "強風注意報".into()
                },
            ]
        );
    }
}
