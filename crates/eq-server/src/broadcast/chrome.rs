//! 画面の無い Chrome を起動してページを開き、DevTools (CDP) の Page.startScreencast で画面の変化を JPEG で受け取る。
//! Chrome は受け取りの確認 (ack) を返すまで次の画面を送らないので、受け取ったら必ず ack する。

use std::process::Stdio;
use std::time::Duration;

use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::sync::mpsc::UnboundedSender;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use super::BroadcastConfig;

/// ページが window.eqBroadcast(json) で知らせてくる口の名前 (mixer の音の知らせ)
const BINDING: &str = "eqBroadcast";

pub struct Screencast {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_id: u64,
    /// ページからの知らせの送り先 (mixer を使うときだけ)
    notices: Option<UnboundedSender<String>>,
}

/// Chrome を起動して画面の受け取りを始める。Chrome のプロセス (止まったか見張る) も返す。
/// url を開く。notices があれば、ページの window.eqBroadcast(json) の json をそこへ送る
pub async fn launch(
    cfg: &BroadcastConfig,
    url: &str,
    notices: Option<UnboundedSender<String>>,
) -> anyhow::Result<(Screencast, Child)> {
    // 普段使いの Chrome のプロファイルとは分ける。指定が無ければ毎回まっさらな一時ディレクトリ
    let profile = if cfg.profile.is_empty() {
        let tmp = std::env::temp_dir().join(format!("eq-broadcast-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        tmp
    } else {
        std::path::PathBuf::from(&cfg.profile)
    };
    let mut child = Command::new(&cfg.chrome)
        .args([
            "--headless=new".to_string(),
            "--remote-debugging-port=0".to_string(),
            format!("--user-data-dir={}", profile.display()),
            format!("--window-size={},{}", cfg.width, cfg.height),
            "--force-device-scale-factor=1".to_string(),
            "--hide-scrollbars".to_string(),
            // 利用者の操作なしで音 (警戒音・BGM) を鳴らす
            "--autoplay-policy=no-user-gesture-required".to_string(),
            // 音の出力先 (&sink=) を名前で探すには、ページがマイクを一度開く必要がある。その確認を自動で許可する
            // (画面の無い Chrome は、マイクを開くまで機器の名前を見せない。開いたマイクはすぐ閉じ、音は使わない)
            "--use-fake-ui-for-media-stream".to_string(),
            "--no-first-run".to_string(),
            "--no-default-browser-check".to_string(),
            // 見えていないページとして動きを間引かれないように
            "--disable-background-timer-throttling".to_string(),
            "--disable-renderer-backgrounding".to_string(),
            "--disable-backgrounding-occluded-windows".to_string(),
            // 知らせの口 (addBinding) を作ってから開く。開いたあとに作ると、読み込み済みのページには付かないことがある
            "about:blank".to_string(),
        ])
        .envs(&cfg.chrome_env)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("starting {}", cfg.chrome))?;
    let mut log = BufReader::new(child.stderr.take().context("chrome stderr")?).lines();
    let browser = tokio::time::timeout(Duration::from_secs(30), async {
        while let Some(l) = log.next_line().await? {
            if let Some(url) = l.strip_prefix("DevTools listening on ") {
                return Ok(url.trim().to_string());
            }
        }
        anyhow::bail!("chrome exited before DevTools was ready")
    })
    .await
    .context("chrome did not start in 30s")??;
    // 残りの出力は読み捨てる (読まないとパイプが詰まる)
    tokio::spawn(async move { while let Ok(Some(_)) = log.next_line().await {} });

    let page = page_url(&browser).await?;
    let (ws, _) = tokio_tungstenite::connect_async(&page)
        .await
        .context("connect to the page")?;
    let mixer = notices.is_some();
    let mut sc = Screencast {
        ws,
        next_id: 0,
        notices,
    };
    // ページの警告 (音の出力先が見つからない、など) をログに出す
    sc.send("Runtime.enable", json!({})).await?;
    if mixer {
        sc.send("Runtime.addBinding", json!({ "name": BINDING })).await?;
    }
    sc.send(
        "Emulation.setDeviceMetricsOverride",
        json!({ "width": cfg.width, "height": cfg.height, "deviceScaleFactor": 1, "mobile": false }),
    )
    .await?;
    sc.send(
        "Page.startScreencast",
        json!({ "format": "jpeg", "quality": 80, "maxWidth": cfg.width, "maxHeight": cfg.height, "everyNthFrame": 1 }),
    )
    .await?;
    sc.send("Page.navigate", json!({ "url": url })).await?;
    Ok((sc, child))
}

/// ブラウザの DevTools の URL (ws://127.0.0.1:PORT/devtools/browser/...) から、開いたページの URL を探す
async fn page_url(browser: &str) -> anyhow::Result<String> {
    let host = browser
        .strip_prefix("ws://")
        .and_then(|r| r.split('/').next())
        .context("unexpected DevTools URL")?;
    let client = crate::net::client(Duration::from_secs(5))?;
    for _ in 0..20 {
        // 起動直後は応答しないことがあるので、失敗してもやり直す
        let targets: Vec<Value> = crate::net::json(client.get(format!("http://{host}/json/list")))
            .await
            .unwrap_or_default();
        let page = targets
            .iter()
            .find(|t| t["type"] == "page")
            .and_then(|t| t["webSocketDebuggerUrl"].as_str());
        if let Some(url) = page {
            return Ok(url.to_string());
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    anyhow::bail!("no page in chrome")
}

impl Screencast {
    async fn send(&mut self, method: &str, params: Value) -> anyhow::Result<()> {
        self.next_id += 1;
        let msg = json!({ "id": self.next_id, "method": method, "params": params });
        self.ws
            .send(Message::text(msg.to_string()))
            .await
            .context(method.to_string())
    }

    /// 次の画面 (base64 の JPEG と、ack に使う番号)。取り消されても受け取り途中の画面を失わない
    /// (tokio::select! の中で使うため、ここでは送信をしない)
    pub async fn next_frame(&mut self) -> anyhow::Result<(String, i64)> {
        loop {
            let msg = self
                .ws
                .next()
                .await
                .context("chrome closed the DevTools connection")??;
            let Message::Text(text) = msg else { continue };
            let v: Value = serde_json::from_str(text.as_str())?;
            if v["method"] == "Runtime.consoleAPICalled"
                && matches!(v["params"]["type"].as_str(), Some("warning" | "error" | "info"))
            {
                let text: Vec<String> = v["params"]["args"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|a| {
                        a["value"]
                            .as_str()
                            .map_or_else(|| a["description"].to_string(), str::to_string)
                    })
                    .collect();
                tracing::warn!("page: {}", text.join(" "));
            }
            if v["method"] == "Runtime.bindingCalled" && v["params"]["name"] == BINDING {
                if let (Some(tx), Some(payload)) = (&self.notices, v["params"]["payload"].as_str()) {
                    let _ = tx.send(payload.to_string());
                }
            }
            if v["method"] == "Page.screencastFrame" {
                let p = &v["params"];
                let data = p["data"].as_str().context("frame without data")?.to_string();
                return Ok((data, p["sessionId"].as_i64().unwrap_or(0)));
            }
        }
    }

    pub async fn ack(&mut self, session: i64) -> anyhow::Result<()> {
        self.send("Page.screencastFrameAck", json!({ "sessionId": session }))
            .await
    }
}

/// base64 (標準の文字) を戻す
pub fn decode_base64(s: &str) -> anyhow::Result<Vec<u8>> {
    let val = |c: u8| -> anyhow::Result<u32> {
        Ok(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => anyhow::bail!("invalid base64"),
        } as u32)
    };
    let bytes = s.trim_end_matches('=').as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let n = chunk
            .iter()
            .try_fold(0u32, |acc, &c| Ok::<_, anyhow::Error>(acc << 6 | val(c)?))?;
        let n = n << (6 * (4 - chunk.len()));
        let got = (chunk.len() * 6) / 8;
        out.extend(&n.to_be_bytes()[1..1 + got]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trip() {
        assert_eq!(decode_base64("c291cmNlOmhhY2ttZQ==").unwrap(), b"source:hackme");
        assert_eq!(decode_base64("YWI=").unwrap(), b"ab");
        assert_eq!(decode_base64("YWJj").unwrap(), b"abc");
        assert!(decode_base64("a*b").is_err());
    }
}
