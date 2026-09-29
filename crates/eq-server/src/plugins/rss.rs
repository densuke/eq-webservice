//! RSS 2.0 フィードを生成・蓄積する。
//! HTTP (`route`) で配信する。蓄積内容は `state_path` に保存し再起動後も引き継ぐ。

use std::path::PathBuf;
use std::sync::Arc;

use crate::quake::{jst, Event};
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RssConfig {
    #[serde(default = "default_route")]
    route: String,
    state_path: Option<PathBuf>,
    #[serde(default = "default_title")]
    title: String,
    #[serde(default)]
    link: String,
    #[serde(default = "default_description")]
    description: String,
    #[serde(default = "default_max_items")]
    max_items: usize,
}

fn default_route() -> String {
    "/feed.xml".into()
}
fn default_title() -> String {
    "地震情報".into()
}
fn default_description() -> String {
    "P2P地震情報から受信した地震・津波情報".into()
}
fn default_max_items() -> usize {
    50
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Item {
    guid: String,
    title: String,
    description: String,
    pub_date_ms: i64,
}

pub struct RssSink {
    cfg: RssConfig,
    /// 新しい順
    items: Mutex<Vec<Item>>,
    /// 生成済みの XML (HTTP 配信用のキャッシュ)
    xml: Arc<Mutex<String>>,
}

impl RssSink {
    pub fn new(cfg: RssConfig) -> anyhow::Result<Self> {
        let items: Vec<Item> = match &cfg.state_path {
            Some(p) if p.exists() => serde_json::from_str(&std::fs::read_to_string(p)?)?,
            _ => Vec::new(),
        };
        let xml = render(&cfg, &items);
        Ok(RssSink {
            cfg,
            items: Mutex::new(items),
            xml: Arc::new(Mutex::new(xml)),
        })
    }

    async fn add(&self, new: impl IntoIterator<Item = &Event>) -> anyhow::Result<()> {
        let mut items = self.items.lock().await;
        let mut changed = false;
        for ev in new {
            if items.iter().any(|i| i.guid == ev.id) {
                continue;
            }
            items.insert(0, to_item(ev));
            changed = true;
        }
        if !changed {
            return Ok(());
        }
        items.sort_by_key(|i| std::cmp::Reverse(i.pub_date_ms));
        items.truncate(self.cfg.max_items);
        let xml = render(&self.cfg, &items);
        if let Some(p) = &self.cfg.state_path {
            write_atomic(p, serde_json::to_string(&*items)?.as_bytes()).await?;
        }
        *self.xml.lock().await = xml;
        Ok(())
    }
}

impl RssSink {
    pub async fn handle(&self, ev: &Event) -> anyhow::Result<()> {
        self.add([ev]).await
    }

    pub async fn seed(&self, events: &[Arc<Event>]) -> anyhow::Result<()> {
        // フィルタは起動処理側で適用済み
        self.add(events.iter().map(|e| e.as_ref())).await
    }

    pub fn routes(&self) -> Router {
        let xml = self.xml.clone();
        Router::new().route(
            &self.cfg.route,
            get(move || async move {
                let xml = xml.lock().await.clone();
                ([(header::CONTENT_TYPE, "application/rss+xml; charset=utf-8")], xml).into_response()
            }),
        )
    }
}

fn to_item(ev: &Event) -> Item {
    Item {
        guid: ev.id.clone(),
        title: ev.title(),
        description: ev.summary(),
        pub_date_ms: ev.issued_at_ms().unwrap_or(ev.received_at_ms as i64),
    }
}

fn render(cfg: &RssConfig, items: &[Item]) -> String {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<rss version=\"2.0\">\n<channel>\n");
    s += &format!("<title>{}</title>\n", esc(&cfg.title));
    s += &format!("<link>{}</link>\n", esc(&cfg.link));
    s += &format!("<description>{}</description>\n", esc(&cfg.description));
    s += "<language>ja</language>\n";
    if let Some(first) = items.first() {
        s += &format!("<lastBuildDate>{}</lastBuildDate>\n", rfc822(first.pub_date_ms));
    }
    for i in items {
        s += "<item>\n";
        s += &format!("  <title>{}</title>\n", esc(&i.title));
        s += &format!(
            "  <description>{}</description>\n",
            // description は「HTML をエスケープして入れる」決まりなので、本文を HTML として
            // エスケープし、改行だけを <br> にしたものを、さらに XML としてエスケープする
            esc(&esc(&i.description).replace('\n', "<br>"))
        );
        if !cfg.link.is_empty() {
            s += &format!("  <link>{}</link>\n", esc(&cfg.link));
        }
        s += &format!("  <guid isPermaLink=\"false\">{}</guid>\n", esc(&i.guid));
        s += &format!("  <pubDate>{}</pubDate>\n", rfc822(i.pub_date_ms));
        s += "</item>\n";
    }
    s += "</channel>\n</rss>\n";
    s
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            c => o.push(c),
        }
    }
    o
}

/// RFC 822 形式 (JST 表記)
fn rfc822(ms: i64) -> String {
    const WD: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MON: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let local = jst::format(ms); // "YYYY/MM/DD HH:MM:SS"
    let days = (ms + 9 * 3_600_000).div_euclid(86_400_000);
    let m: usize = local[5..7].parse().unwrap_or(1);
    format!(
        "{}, {} {} {} {} +0900",
        WD[days.rem_euclid(7) as usize],
        &local[8..10],
        MON[m - 1],
        &local[0..4],
        &local[11..19]
    )
}

async fn write_atomic(path: &std::path::Path, data: &[u8]) -> anyhow::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        tokio::fs::create_dir_all(dir).await?;
    }
    let tmp = path.with_extension("tmp");
    tokio::fs::write(&tmp, data).await?;
    tokio::fs::rename(&tmp, path).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn description_is_html_inside_xml() {
        let cfg: RssConfig = toml::from_str("").unwrap();
        let item = Item {
            guid: "g".into(),
            title: "t".into(),
            description: "a<b\nc".into(),
            pub_date_ms: 0,
        };
        let xml = render(&cfg, &[item]);
        // XML として読むと "a&lt;b<br>c" (HTML として a<b 改行 c) になる
        assert!(
            xml.contains("<description>a&amp;lt;b&lt;br&gt;c</description>"),
            "{xml}"
        );
    }

    #[test]
    fn rfc822_date() {
        // 2026-09-28 16:24:00 JST は月曜日
        assert_eq!(rfc822(1_790_580_240_000), "Mon, 28 Sep 2026 16:24:00 +0900");
    }

    #[tokio::test]
    async fn persists_and_renders() {
        let dir = tempfile::tempdir().unwrap();
        let cfg: RssConfig = toml::from_str(&format!(
            "state_path = {:?}\nmax_items = 1",
            dir.path().join("state.json")
        ))
        .unwrap();
        let sink = RssSink::new(cfg).unwrap();
        let ev = |id: &str, t: &str| {
            crate::quake::p2pquake::parse(&format!(
                r#"{{"code":551,"id":"{id}","issue":{{"time":"{t}","type":"ScalePrompt"}},
                   "earthquake":{{"time":"{t}","maxScale":30,"domesticTsunami":"None"}},
                   "points":[{{"pref":"東京都","addr":"23区","isArea":true,"scale":30}}]}}"#
            ))
            .unwrap()
            .unwrap()
        };
        sink.handle(&ev("a", "2026/09/28 10:00:00")).await.unwrap();
        sink.handle(&ev("b<&>", "2026/09/28 11:00:00")).await.unwrap();
        let xml = sink.xml.lock().await.clone();
        assert!(xml.contains("<guid isPermaLink=\"false\">b&lt;&amp;&gt;</guid>"));
        assert!(!xml.contains(">a</guid>"), "max_items で古いものは消える");

        // 再起動しても引き継がれる
        let cfg: RssConfig = toml::from_str(&format!("state_path = {:?}", dir.path().join("state.json"))).unwrap();
        let sink = RssSink::new(cfg).unwrap();
        assert_eq!(sink.items.lock().await.len(), 1);
    }
}
