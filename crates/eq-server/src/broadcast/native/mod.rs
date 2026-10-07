//! 配信の画面を Chrome を使わずに Rust で描く (docs/broadcast-native.md)。
//! サーバから地震情報 (WebSocket)・警報・天気を受け取り、変化したときだけ 1280x720 の RGBA を描き直して
//! watch で渡す (I420 に変換済み。fps に合わせて同じ画面を繰り返し送るのは、呼ぶ側の時計)。
//! 平時と地震の画面の切り替えに合わせて、mixer に BGM を流す・止める知らせを出す。

mod banner;
mod calm;
mod camera;
mod cards;
mod chip;
mod data;
mod draw;
mod eew;
mod frame;
mod geo;
mod held;
mod hindsight;
mod icon;
mod layout_def;
mod layout_resolve;
pub(crate) mod model;
mod notice;
mod paint;
mod panel;
mod placed;
mod quake_band;
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
use super::status::Feed;
use super::BroadcastConfig;
use crate::broadcast::mixer::AlertLevel;
use crate::quake::userquake::Gate;
use crate::quake::{Event, EventBody};
use data::{CityWeather, ServerMessage, Warnings};
use draw::Renderer;
use notice::{Banners, Notices};
use placed::Placed;

// 記録から描き直す動画 (broadcast/replay) が使う
pub(super) use eew::{eew_place, latest_eews, quake_place, EEW_ACTIVE_MS};
pub(super) use hindsight::{hindsight_of, Hindsight};
pub(super) use icon::Icons;
pub(super) use model::{distance_km, event_place, group_quakes, same_quake, Place};
pub(super) use step::{look, Input, Output, Stepper};

const RECONNECT_AFTER: Duration = Duration::from_secs(5);
/// 警報・天気を取り直す間隔 (取れなかったときは短く)
const POLL_EVERY: Duration = Duration::from_secs(300);
/// お知らせを取り直す間隔 (サーバは毎回ディレクトリを読み直すので、差し替えはこの間隔で入る)
const NOTICE_EVERY: Duration = Duration::from_secs(60);
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
    /// 右パネルの下に出すお知らせ (取れていなければ None)
    notices: Option<Notices>,
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

/// 並びの定義の取得を試す回数と間隔 (サーバの起動待ち)
const LAYOUT_TRIES: u32 = 5;
const LAYOUT_RETRY: Duration = Duration::from_secs(2);

/// session を始めるたびに (やり直しのたびに) 1 回、サーバの並びの定義 (GET /api/layout) を読み、平時用と地震の画面用の 2 つを割り付ける。
/// 取れない・使えないときは組み込みの定義 (警告は読むたびに出る)。組み込みも使えなければエラー
pub async fn load_layouts(cfg: &BroadcastConfig) -> anyhow::Result<Placed> {
    let url = format!("{}/api/layout", cfg.server.trim_end_matches('/'));
    // eq-server と同時に再起動される (PartOf) ので、サーバの起動を少し待つ。2 秒おきに 5 回まで試す
    let mut json = Err(anyhow::anyhow!("まだ読んでいない"));
    for attempt in 0..LAYOUT_TRIES {
        if attempt > 0 {
            tokio::time::sleep(LAYOUT_RETRY).await;
        }
        json = async {
            let client = crate::net::client(Duration::from_secs(10))?;
            crate::net::text(client.get(&url)).await
        }
        .await;
        if json.is_ok() {
            break;
        }
    }
    if let Err(e) = &json {
        tracing::warn!("broadcast: {url}: {e:#}");
    }
    let defs = layout_def::load(json.ok().as_deref(), &cfg.layout, &cfg.layout_quake)?;
    tracing::info!("broadcast: layout 平時={} 地震={}", defs.0.name, defs.1.name);
    for def in [&defs.0, &defs.1] {
        let skipped = layout_resolve::unsupported_slots(def);
        if !skipped.is_empty() {
            tracing::info!("broadcast: layout {} の部品 {skipped:?} は配信では描きません", def.name);
        }
    }
    // 割り付けに失敗する定義 (検査をすり抜けた分) でも配信は止めず、組み込みの定義に戻す
    Placed::new(&defs.0, &defs.1).or_else(|e| {
        tracing::warn!("broadcast: layout を割り付けられないので、組み込みの定義にします: {e:#}");
        Placed::builtin()
    })
}

pub fn start(
    cfg: &BroadcastConfig,
    placed: Placed,
    notices: Option<UnboundedSender<String>>,
    status: Feed,
) -> anyhow::Result<Native> {
    let renderer = load_renderer_with(cfg, placed)?;
    let server = cfg.server.trim_end_matches('/').to_string();
    let st: Shared = Arc::default();
    let (tx, frames) = watch::channel(Arc::new(Vec::new()));
    let (calm_tx, calm) = watch::channel(true);
    let (shown_tx, shown) = watch::channel(Shown::NONE);
    let (label, test) = (cfg.label.clone(), cfg.test);
    // 描き直すかを調べる間隔。地震波が動く間はこの間隔ごとに描き直すので、地震の画面の fps に合わせる
    let check_ms = 1000 / u64::from(cfg.fps.max(1));
    let tasks = vec![
        tokio::spawn(ws_loop(
            ws_url(&server),
            server.clone(),
            cfg.voice,
            notices.clone(),
            st.clone(),
        )),
        tokio::spawn(poll(format!("{server}/api/warnings"), st.clone(), |s, v: Warnings| {
            s.warnings = Some(v)
        })),
        tokio::spawn(poll(
            format!("{server}/api/weather"),
            st.clone(),
            |s, v: CityWeather| s.weather = Some(v),
        )),
        tokio::spawn(notice_loop(format!("{server}/api/banners"), st.clone())),
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
                status,
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

/// 組み込みの定義の並びで作る (再現動画とテスト)
pub(super) fn load_renderer(cfg: &BroadcastConfig) -> anyhow::Result<Renderer> {
    load_renderer_with(cfg, Placed::builtin()?)
}

fn load_renderer_with(cfg: &BroadcastConfig, placed: Placed) -> anyhow::Result<Renderer> {
    let view = geo::View::fit_home(placed.main.tuple64());
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
    let mut renderer = Renderer::new(view, neighbors, prefs, areas, text, placed);
    if cfg.zoom || cfg.sub_map {
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
        renderer.set_zones(zones, stations);
    }
    if cfg.zoom {
        renderer.enable_zoom();
    }
    if cfg.sub_map {
        renderer.enable_sub_map();
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
    /// 状態の札の材料 (ライブだけが持つ)
    status: Feed,
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
            let status = out.status.notice(local_now_ms());
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
                status,
                notices: s.notices.as_ref(),
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

async fn ws_loop(url: String, base: String, voice: bool, notices: Option<UnboundedSender<String>>, st: Shared) {
    loop {
        if let Err(e) = ws_once(&url, &base, voice, notices.as_ref(), &st).await {
            tracing::warn!("broadcast: ws {e:#}");
        }
        change(&st, |s| s.connected = false);
        tokio::time::sleep(RECONNECT_AFTER).await;
    }
}

async fn ws_once(
    url: &str,
    base: &str,
    voice: bool,
    notices: Option<&UnboundedSender<String>>,
    st: &Shared,
) -> anyhow::Result<()> {
    // この接続で届いた報 (到着順)。hello の分は文脈だけで、音は鳴らさない
    let mut live: Vec<Event> = Vec::new();
    let mut gate = Gate::default();
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.context("connect")?;
    change(st, |s| s.connected = true);
    while let Some(msg) = ws.next().await {
        match msg? {
            Message::Text(t) => match serde_json::from_str::<ServerMessage>(t.as_str()) {
                Ok(m) => {
                    if let Some(n) = notices {
                        sound_for(&mut live, &mut gate, &m, base, voice, n);
                    }
                    change(st, |s| apply(s, m))
                }
                Err(e) => tracing::warn!("broadcast: unknown ws message: {e}"),
            },
            Message::Close(_) => break,
            _ => {}
        }
    }
    anyhow::bail!("closed")
}

/// 直近に覚えておく報の数
const MAX_LIVE: usize = 200;

/// 届いた報を覚え、新しい報 (event) なら警戒音と読み上げを mixer に知らせる (hello は覚えるだけ)。
/// voice が false なら読み上げは知らせない (サーバの tts が無効のとき。BroadcastConfig.voice)
fn sound_for(
    live: &mut Vec<Event>,
    gate: &mut Gate,
    m: &ServerMessage,
    base: &str,
    voice: bool,
    notices: &UnboundedSender<String>,
) {
    let (is_new, now_ms, values) = match m {
        ServerMessage::Hello { server_time_ms, events } => (false, *server_time_ms, events.clone()),
        ServerMessage::Event { server_time_ms, event } => (true, *server_time_ms, vec![event.clone()]),
    };
    for ev in data::parse_events(values) {
        live.push(ev);
        if live.len() > MAX_LIVE {
            live.remove(0);
        }
        if !is_new {
            continue;
        }
        let Some((last, before)) = live.split_last() else {
            continue;
        };
        // 地震感知情報は、信頼できる評価を 1 回だけ、案内音のあとに読む (voice のときだけ。docs/tts.md S12)
        if let EventBody::Userquake(u) = &last.body {
            let official = official_received_ms(before);
            if voice && gate.should_read(u, now_ms as i64, &official) {
                let _ = notices.send(model::alert_notice(AlertLevel::Info));
                let _ = notices.send(model::voice_notice(&model::voice_url(base, &last.id)));
            }
            continue;
        }
        if let Some((level, id)) = model::live_alert(live, now_ms) {
            let _ = notices.send(model::alert_notice(level));
            if voice {
                let _ = notices.send(model::voice_notice(&model::voice_url(base, &id)));
            }
        } else if voice && model::should_read_without_alert(before, last) {
            let _ = notices.send(model::voice_notice(&model::voice_url(base, &last.id)));
        }
    }
}

/// 届いた気象庁の地震の情報 (緊急地震速報・地震情報) の受信時刻
fn official_received_ms(events: &[Event]) -> Vec<i64> {
    events
        .iter()
        .filter(|e| matches!(e.body, EventBody::Eew(_) | EventBody::Quake(_)))
        .map(|e| e.received_at_ms as i64)
        .collect()
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

/// 1 分ごとにお知らせ (/api/banners) を取る。失敗しても前の一覧を残し、警告は続けて失敗した最初の 1 回だけ出す
async fn notice_loop(url: String, st: Shared) {
    let Ok(client) = crate::net::client(Duration::from_secs(15)) else {
        return;
    };
    let mut failing = false;
    loop {
        match crate::net::json::<Banners>(client.get(&url)).await {
            Ok(b) => {
                failing = false;
                let n = b.notices();
                change(&st, |s| s.notices = Some(n));
            }
            Err(e) => {
                if !failing {
                    tracing::warn!("broadcast: {url}: {e:#}");
                }
                failing = true;
            }
        }
        tokio::time::sleep(NOTICE_EVERY).await;
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
mod band_tests;
#[cfg(test)]
mod layout_def_tests;
#[cfg(test)]
mod layout_resolve_tests;
#[cfg(test)]
mod sound_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod zoom_tests;
