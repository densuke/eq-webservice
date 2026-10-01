//! 配信の画面を Chrome を使わずに Rust で描く (docs/broadcast-native.md)。
//! サーバから地震情報 (WebSocket)・警報・天気を受け取り、変化したときだけ 1280x720 の RGBA を描き直して
//! watch で渡す (I420 に変換済み。fps に合わせて同じ画面を繰り返し送るのは、呼ぶ側の時計)。
//! 平時と地震の画面の切り替えに合わせて、mixer に BGM を流す・止める知らせを出す。

mod banner;
mod calm;
mod camera;
mod data;
mod draw;
mod eew;
mod frame;
mod geo;
mod held;
mod hindsight;
mod icon;
mod model;
mod paint;
mod panel;
mod shaken;
mod step;
mod telops;
mod test_mark;
mod text;
mod yuv;

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use futures_util::StreamExt;
use serde::de::DeserializeOwned;
use tokio::sync::{mpsc::UnboundedSender, watch};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

use super::record::Shown;
use super::BroadcastConfig;
use crate::quake::Event;
use data::{CityWeather, ServerMessage, Warnings};
use draw::Renderer;

// 記録から描き直す動画 (broadcast/replay) が使う
pub(super) use eew::{eew_place, latest_eews, quake_place, EEW_ACTIVE_MS};
pub(super) use hindsight::{hindsight_of, Hindsight};
pub(super) use icon::Icons;
pub(super) use model::{distance_km, event_place, group_quakes, same_quake, Place};
pub(super) use step::{look, Input, Output, Stepper};

const RECONNECT_AFTER: Duration = Duration::from_secs(5);
/// 警報・天気を取り直す間隔 (取れなかったときは短く)
const POLL_EVERY: Duration = Duration::from_secs(300);
const POLL_RETRY: Duration = Duration::from_secs(30);
/// 足りないアイコンが無いか調べる間隔 (天気が新しくなって、知らないコードが来たときのため)
const ICON_CHECK: Duration = Duration::from_secs(5);
const ICON_RETRY: Duration = Duration::from_secs(60);
const BGM_TITLE_EVERY: Duration = Duration::from_secs(15);
/// 覚えておく地震情報の数
const MAX_EVENTS: usize = 300;
/// 履歴に出す地震の数
const HISTORY: usize = 5;

/// 取得したデータ。変わるたびに rev を進める (描き直しの合図)
#[derive(Default)]
struct State {
    events: Vec<Event>,
    /// サーバの時計 - 自分の時計 (ミリ秒)
    offset: i64,
    warnings: Option<Warnings>,
    weather: Option<CityWeather>,
    icons: Icons,
    bgm_title: String,
    connected: bool,
    rev: u64,
}

type Shared = Arc<Mutex<State>>;

fn change(st: &Shared, f: impl FnOnce(&mut State)) {
    let mut s = st.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut s);
    s.rev += 1;
}

fn local_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// 動かしている画面。捨てると、データを取るタスクも止まる
pub struct Native {
    pub frames: watch::Receiver<Arc<Vec<u8>>>,
    /// 平時か (true) 地震の画面か (false)。切り替わったときに変わる
    pub calm: watch::Receiver<bool>,
    /// 出している地震の最大震度・警報か (record の判断用。calm より先に送る)
    pub shown: watch::Receiver<Shown>,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for Native {
    fn drop(&mut self) {
        self.tasks.iter().for_each(JoinHandle::abort);
    }
}

/// 取得先が記録の再生 (replay) なのに test = true でないときは、配信を始めさせない
/// (過去の地震を本番の配信に流して、実際の地震と取り違えられないように)。
/// 取れない (古いサーバなど) ときは replay ではないとみなす
pub async fn check_source(cfg: &BroadcastConfig) -> anyhow::Result<()> {
    let url = format!("{}/api/source", cfg.server.trim_end_matches('/'));
    let kind = async {
        let client = crate::net::client(Duration::from_secs(10))?;
        crate::net::json::<serde_json::Value>(client.get(&url)).await
    }
    .await
    .ok()
    .and_then(|v| v.get("type")?.as_str().map(str::to_string));
    check_replay(kind.as_deref(), cfg.test)
}

fn check_replay(kind: Option<&str>, test: bool) -> anyhow::Result<()> {
    if kind == Some("replay") && !test {
        return Err(super::Refused(
            "データの取得先が記録の再生 (replay) です。テスト配信として test = true を設定してください \
             (送り先はテスト用の別の配信のキーにすること)"
                .into(),
        )
        .into());
    }
    Ok(())
}

pub fn start(cfg: &BroadcastConfig, notices: Option<UnboundedSender<String>>) -> anyhow::Result<Native> {
    let renderer = load_renderer(cfg)?;
    let server = cfg.server.trim_end_matches('/').to_string();
    let st: Shared = Arc::default();
    let (tx, frames) = watch::channel(Arc::new(Vec::new()));
    let (calm_tx, calm) = watch::channel(true);
    let (shown_tx, shown) = watch::channel(Shown::NONE);
    let (label, test) = (cfg.label.clone(), cfg.test);
    // 描き直すかを調べる間隔。地震波が動く間はこの間隔ごとに描き直すので、地震の画面の fps に合わせる
    let check_ms = 1000 / u64::from(cfg.fps.max(1));
    let tasks = vec![
        tokio::spawn(ws_loop(ws_url(&server), st.clone())),
        tokio::spawn(poll(format!("{server}/api/warnings"), st.clone(), |s, v: Warnings| {
            s.warnings = Some(v)
        })),
        tokio::spawn(poll(
            format!("{server}/api/weather"),
            st.clone(),
            |s, v: CityWeather| s.weather = Some(v),
        )),
        tokio::spawn(icon_loop(icon::IMG_BASE.to_string(), st.clone())),
        tokio::spawn(bgm_title_loop(format!("{server}/stream/status-json.xsl"), st.clone())),
        tokio::spawn(render_loop(
            renderer,
            st,
            Out {
                frames: tx,
                calm: calm_tx,
                shown: shown_tx,
                label,
                test,
                check_ms,
                flip_s: cfg.weather_flip_secs,
            },
            notices,
        )),
    ];
    Ok(Native {
        frames,
        calm,
        shown,
        tasks,
    })
}

pub(super) fn load_renderer(cfg: &BroadcastConfig) -> anyhow::Result<Renderer> {
    let view = geo::View::fit_home(draw::MAP_RECT);
    let dir = std::path::Path::new(&cfg.map_dir);
    // 周辺国の陸地は背景なので、無くても続ける
    let neighbors = geo::load(&dir.join("neighbors.geojson"), "name", &view).unwrap_or_else(|e| {
        tracing::warn!("broadcast: 周辺国の陸地を読めないので、描きません: {e:#}");
        Vec::new()
    });
    let prefs = geo::load(&dir.join("japan.geojson"), "name", &view)?;
    let areas = geo::load(&dir.join("warning-areas.geojson"), "code", &view)?;
    let text = text::Text::load(&cfg.font, cfg.font_index).unwrap_or_else(|e| {
        tracing::warn!("broadcast: font {} を読めないので、文字は描きません: {e:#}", cfg.font);
        text::Text::none()
    });
    let mut renderer = Renderer::new(view, neighbors, prefs, areas, text);
    if cfg.zoom {
        // 地震情報細分区域は寄りの範囲の計算だけに使う。読めなければ、県の本土の範囲で寄る
        let zones = geo::load(&dir.join("areas.geojson"), "name", &view).unwrap_or_else(|e| {
            tracing::warn!("broadcast: 地震情報細分区域を読めないので、県の範囲で寄ります: {e:#}");
            Vec::new()
        });
        // 観測点の表は、各地の震度の観測点から区域を引くのに使う。読めなければ区域で届いた分だけで寄る
        let stations = shaken::load_stations(&dir.join("stations.json")).unwrap_or_else(|e| {
            tracing::warn!("broadcast: 観測点の表を読めないので、観測点では寄りません: {e:#}");
            Default::default()
        });
        renderer.enable_zoom(zones, stations);
    }
    Ok(renderer)
}

/// https://host → wss://host/ws
fn ws_url(server: &str) -> String {
    let (scheme, rest) = server.split_once("://").unwrap_or(("https", server));
    format!("{}://{rest}/ws", if scheme == "http" { "ws" } else { "wss" })
}

/// render_loop の出力 (画面・平時かどうか) と、上部バーに出す名前
struct Out {
    frames: watch::Sender<Arc<Vec<u8>>>,
    calm: watch::Sender<bool>,
    shown: watch::Sender<Shown>,
    label: String,
    test: bool,
    check_ms: u64,
    /// 天気の札を今と明日で切り替える間隔 (秒。0 は今だけ)
    flip_s: u64,
}

async fn render_loop(renderer: Renderer, st: Shared, out: Out, notices: Option<UnboundedSender<String>>) {
    let mut stepper = Stepper::new(renderer);
    let mut last_calm = None;
    let mut tick = tokio::time::interval(Duration::from_millis(out.check_ms));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        let o = {
            let s = st.lock().unwrap_or_else(|e| e.into_inner());
            stepper.step(&Input {
                events: &s.events,
                now: model::server_now(local_now_ms(), s.offset),
                rev: s.rev,
                warnings: s.warnings.as_ref(),
                weather: s.weather.as_ref(),
                icons: &s.icons,
                bgm_title: &s.bgm_title,
                connected: s.connected,
                label: &out.label,
                test: out.test,
                check_ms: out.check_ms,
                flip_s: out.flip_s,
                hindsight: None,
                fast_forward: false,
            })
        };
        let Some(Output { i420, calm, shown }) = o else {
            continue;
        };
        // calm より先に送る (record は calm が変わったときに読む)
        let _ = out.shown.send_if_modified(|c| std::mem::replace(c, shown) != shown);
        let _ = out.calm.send_if_modified(|c| std::mem::replace(c, calm) != calm);
        let _ = out.frames.send(Arc::new(i420));
        if last_calm != Some(calm) {
            last_calm = Some(calm);
            if let Some(n) = &notices {
                let _ = n.send(model::bgm_notice(calm));
            }
        }
    }
}

async fn ws_loop(url: String, st: Shared) {
    loop {
        if let Err(e) = ws_once(&url, &st).await {
            tracing::warn!("broadcast: ws {e:#}");
        }
        change(&st, |s| s.connected = false);
        tokio::time::sleep(RECONNECT_AFTER).await;
    }
}

async fn ws_once(url: &str, st: &Shared) -> anyhow::Result<()> {
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.context("connect")?;
    change(st, |s| s.connected = true);
    while let Some(msg) = ws.next().await {
        match msg? {
            Message::Text(t) => match serde_json::from_str::<ServerMessage>(t.as_str()) {
                Ok(m) => change(st, |s| apply(s, m)),
                Err(e) => tracing::warn!("broadcast: unknown ws message: {e}"),
            },
            Message::Close(_) => break,
            _ => {}
        }
    }
    anyhow::bail!("closed")
}

/// メッセージを状態に反映する (時計のずれを覚え、地震情報を id で重複なく足す)
fn apply(s: &mut State, m: ServerMessage) {
    let (server_ms, new) = match m {
        ServerMessage::Hello { server_time_ms, events } => (server_time_ms, data::parse_events(events)),
        ServerMessage::Event { server_time_ms, event } => (server_time_ms, data::parse_events(vec![event])),
    };
    s.offset = model::clock_offset(server_ms, local_now_ms());
    for e in new {
        if !s.events.iter().any(|x| x.id == e.id) {
            s.events.push(e);
        }
    }
    if s.events.len() > MAX_EVENTS {
        s.events.sort_by_key(|e| std::cmp::Reverse(e.received_at_ms));
        s.events.truncate(MAX_EVENTS);
    }
}

/// 5 分ごとに JSON を取る (まだ無い (null) ときや失敗したときは早めにやり直す)
async fn poll<T: DeserializeOwned>(url: String, st: Shared, set: fn(&mut State, T)) {
    let Ok(client) = crate::net::client(Duration::from_secs(15)) else {
        return;
    };
    loop {
        let wait = match crate::net::json::<Option<T>>(client.get(&url)).await {
            Ok(Some(v)) => {
                change(&st, |s| set(s, v));
                POLL_EVERY
            }
            Ok(None) => POLL_RETRY,
            Err(e) => {
                tracing::warn!("broadcast: {url}: {e:#}");
                POLL_RETRY
            }
        };
        tokio::time::sleep(wait).await;
    }
}

/// 天気のアイコン (昼・夜の両方) を、足りないものだけ取って覚える。取れなかったものは 60 秒後にやり直す
async fn icon_loop(base: String, st: Shared) {
    let Ok(client) = crate::net::client(Duration::from_secs(10)) else {
        return;
    };
    loop {
        let missing = {
            let s = st.lock().unwrap_or_else(|e| e.into_inner());
            let codes = s
                .weather
                .iter()
                .flat_map(|w| &w.cities)
                .flat_map(|c| [Some(c.code.as_str()), c.tomorrow.as_ref().map(|t| t.code.as_str())])
                .flatten()
                .filter_map(icon::names);
            let mut names: Vec<&str> = codes
                .flat_map(|(d, n)| [d, n])
                .filter(|n| !s.icons.contains_key(*n))
                .collect();
            names.sort_unstable();
            names.dedup();
            names.into_iter().map(str::to_string).collect::<Vec<_>>()
        };
        let mut failed = false;
        for name in missing {
            match fetch_icon(&client, &base, &name).await {
                Ok(pm) => change(&st, |s| {
                    s.icons.insert(name, pm);
                }),
                Err(e) => {
                    tracing::warn!("broadcast: icon {name}: {e:#}");
                    failed = true;
                }
            }
        }
        tokio::time::sleep(if failed { ICON_RETRY } else { ICON_CHECK }).await;
    }
}

async fn fetch_icon(client: &reqwest::Client, base: &str, name: &str) -> anyhow::Result<tiny_skia::Pixmap> {
    let svg = crate::net::body(client.get(format!("{base}{name}"))).await?;
    icon::rasterize(&svg).context("drawing svg")
}

async fn bgm_title_loop(url: String, st: Shared) {
    let Ok(client) = crate::net::client(Duration::from_secs(10)) else {
        return;
    };
    loop {
        let title = crate::net::json::<serde_json::Value>(client.get(&url))
            .await
            .ok()
            .and_then(|v| data::bgm_title(&v))
            .unwrap_or_default();
        let same = st.lock().unwrap_or_else(|e| e.into_inner()).bgm_title == title;
        if !same {
            change(&st, |s| s.bgm_title = title);
        }
        tokio::time::sleep(BGM_TITLE_EVERY).await;
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod zoom_tests;
