//! ライブ配信 (`eq-server broadcast <broadcast.toml>`)。画面の無い Chrome で地図のページを開き、
//! 画面の変化を受け取って (chrome.rs)、決まった fps で ffmpeg に渡して配信する。
//! 音は mixer (eq-server 自身が BGM と警戒音を混ぜる) か、ffmpeg の入力 (Mac は BlackHole、Linux は PulseAudio のモニタなど) で取り込む。
//! Chrome か ffmpeg が止まったら、両方を止めて少し待ってから立ち上げ直す。
//! 送り先 (ストリームキー入りの URL) は `$VAR` で環境変数から読み、ログには出さない。

mod audio;
mod builtin;
mod calm_state;
mod chrome;
mod encoder;
mod ffmpeg;
mod mixer;
pub(crate) mod native;
mod psi;
mod record;
mod replay;
mod ring;
mod status;

pub use replay::{run as replay_video, run_worker as replay_worker};

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use encoder::Encoder;
use serde::Deserialize;

/// 圧縮と送り出しの方法
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EncoderKind {
    /// ffmpeg の子のプロセス (既定)
    Ffmpeg,
    /// eq-server の中で圧縮して送る (実験。native・無音のみ。docs/broadcast-builtin.md)
    Builtin,
}

/// 画面の作り方
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// 画面の無い Chrome でページを開き、JPEG で受け取る (今までの動き)
    Chrome,
    /// Rust で描く (native/。Chrome は起動しない)
    Native,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct BroadcastConfig {
    /// 画面の作り方 (既定は chrome)
    pub source: Source,
    /// native: データの取得先 (地震情報・警報・天気・BGM の曲名)
    pub server: String,
    /// native: 文字のフォント (.ttc は font_index 番目)。読めなければ文字を描かない
    pub font: String,
    pub font_index: u32,
    /// native: 地図のデータ (japan.geojson・warning-areas.geojson) の場所
    pub map_dir: String,
    /// 開くページ (配信用の表示は ?broadcast=1)
    pub url: String,
    pub width: u32,
    pub height: u32,
    /// native: 地震の画面のときのコマ数 (Chrome は常にこの値)
    pub fps: u32,
    /// native: 平時のコマ数。指定すると、ffmpeg にはコマが届いた時刻で渡し、平時と地震でコマの間隔を変える。省けば fps で一定
    pub fps_calm: Option<u32>,
    /// native: 地震のとき震源へ寄る (S 波の広がりに合わせて引き、揺れた範囲が収まったら止まる。web と同じ)。
    /// 描き直しが増える (e2-micro では測ってから)。既定は寄らない
    pub zoom: bool,
    /// native: 平時の並びを決める定義の名前 (サーバの GET /api/layout の中の名前。web/src/layout.json の broadcast)
    pub layout: String,
    /// native: 地震の画面の並びを決める定義の名前
    pub layout_quake: String,
    /// native 試験: 右パネルの上にサブの地図 (表示中の地震、無ければ最新の地震に寄せたもの) を描く (重さを測るため。docs/native-submap-bench.md)。既定は無効
    pub sub_map: bool,
    /// native: 上部バーの右に出す配信元の名前 (例 "配信元: e2")。空なら出さない
    pub label: String,
    /// native: 平時の天気の札を「今」と「明日」で切り替える間隔 (秒)。0 なら今だけ
    pub weather_flip_secs: u64,
    /// native: テスト配信 (過去の地震の再生など)。赤い帯・TEST の透かし・[テスト] を必ず描く。replay のサーバに向けるときは必須
    pub test: bool,
    /// Chrome の実行ファイル
    pub chrome: String,
    /// Chrome に渡す環境変数 (Linux で音の出力先を決める PULSE_SINK など)
    pub chrome_env: BTreeMap<String, String>,
    /// Chrome のプロファイルの場所 (普段使いと分ける)。空なら毎回まっさらな一時ディレクトリ
    pub profile: String,
    pub ffmpeg: String,
    /// 音の入力 (ffmpeg の引数)。空なら無音
    pub audio: Vec<String>,
    /// 音を取り込むコマンド (sox など)。指定すると audio より優先し、その標準出力 (s16le・48kHz・ステレオ) を使う
    pub audio_command: Vec<String>,
    /// 音を eq-server の中で作る (BGM と警戒音を混ぜて ffmpeg に渡す。BlackHole・sox が要らない)。
    /// 指定すると audio・audio_command より優先し、ページには &audio=mixer を付けて開く
    pub mixer: bool,
    /// 新しい報を読み上げる (native と mixer のとき。サーバの [tts] が有効で、GOOGLE_TTS_API_KEY があるときだけ true にする。
    /// 無効のサーバに頼むと 404 になるので、既定は false)
    pub voice: bool,
    /// mixer が流す BGM (Icecast の MP3)。空なら BGM は流さない
    pub bgm_url: String,
    /// 音のビットレート (ffmpeg の -b:a。無音なら 32k などに下げる)
    pub audio_bitrate: String,
    /// 圧縮と送り出し (既定は ffmpeg)。builtin は native・無音のときだけ。output の最初の要素 (rtmp(s)://... か .flv のパス) に送る
    pub encoder: EncoderKind,
    /// builtin: 映像の目標ビットレート (bps)
    pub builtin_bitrate: u32,
    /// 映像・音の圧縮 (ffmpeg の引数)
    pub encode: Vec<String>,
    /// 送り先 (ffmpeg の引数)。`$VAR` / `${VAR}` は環境変数に置き換える。
    /// `[record]` があるときは、送り出し用の ffmpeg (`-c copy`) の引数 (例: `-f flv rtmps://...`)
    pub output: Vec<String>,
    /// native: 地震の画面か平時かを書く状態のファイル (再現動画を作る係が読む。空なら `$XDG_STATE_HOME/eq-broadcast/state.json`)
    pub state_file: String,
    /// 地震の画面になったときの録画の切り出し (native・ffmpeg のときだけ。ring は eq-server が mpegts を受けて書く)。省けば何もしない
    pub record: Option<record::RecordConfig>,
}

/// 既定の圧縮 (コマの間隔が一定のとき)。3000k で頭打ちにする
fn default_encode() -> Vec<String> {
    [
        "-c:v", "libx264", "-preset", "veryfast", "-b:v", "3000k", "-maxrate", "3000k", "-bufsize", "6000k",
    ]
    .map(String::from)
    .to_vec()
}

/// 可変 fps (fps_calm) のときの既定の圧縮。CRF だけで上限を付けない。
/// 入力を届いた時刻で渡すと ffmpeg は入力を 25fps とみなすので、-maxrate を付けると x264 の VBV が
/// 1 コマあたり maxrate/25 に絞り、2fps の平時の画面が崩れる (CRF を変えても送信量が同じになる)
fn variable_fps_encode() -> Vec<String> {
    ["-c:v", "libx264", "-preset", "veryfast", "-crf", "23"]
        .map(String::from)
        .to_vec()
}

impl Default for BroadcastConfig {
    fn default() -> Self {
        BroadcastConfig {
            source: Source::Chrome,
            server: "https://eq.fuga.jp".into(),
            font: if cfg!(target_os = "macos") {
                "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc".into()
            } else {
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc".into()
            },
            font_index: 0,
            map_dir: "web/public".into(),
            url: "https://eq.fuga.jp/?broadcast=1".into(),
            width: 1280,
            height: 720,
            fps: 30,
            fps_calm: None,
            zoom: false,
            layout: "broadcast".into(),
            layout_quake: "broadcast-quake".into(),
            sub_map: false,
            label: String::new(),
            weather_flip_secs: 20,
            test: false,
            chrome: if cfg!(target_os = "macos") {
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into()
            } else {
                "chromium".into()
            },
            chrome_env: BTreeMap::new(),
            profile: String::new(),
            ffmpeg: "ffmpeg".into(),
            audio: Vec::new(),
            audio_command: Vec::new(),
            mixer: false,
            voice: false,
            bgm_url: "https://eq.fuga.jp/stream/bgm.mp3".into(),
            audio_bitrate: "128k".into(),
            encoder: EncoderKind::Ffmpeg,
            builtin_bitrate: 300_000,
            encode: default_encode(),
            output: Vec::new(),
            state_file: String::new(),
            record: None,
        }
    }
}

/// 止まってから立ち上げ直すまで
const RESTART_AFTER: Duration = Duration::from_secs(5);
/// 設定が安全でなくて始めなかったとき (Refused) に、次を試すまで。すぐには繰り返さない
const REFUSED_WAIT: Duration = Duration::from_secs(600);

/// 安全装置が配信を始めさせなかった (設定を直すまで、何度試しても同じ)
#[derive(Debug)]
struct Refused(String);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Refused {}

pub async fn run(args: &[String]) -> anyhow::Result<()> {
    let [path] = args else {
        anyhow::bail!("usage: eq-server broadcast <broadcast.toml>");
    };
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
    let cfg: BroadcastConfig = toml::from_str(&text).with_context(|| format!("parsing {path}"))?;
    anyhow::ensure!(!cfg.output.is_empty(), "output (送り先) を設定してください");
    if cfg.encoder == EncoderKind::Builtin {
        builtin::BuiltinEncoder::check(&cfg)?;
    }
    if cfg.encoder == EncoderKind::Ffmpeg
        && cfg.source == Source::Native
        && cfg.fps_calm.is_some()
        && cfg.encode.iter().any(|x| x == "-maxrate")
    {
        tracing::warn!(
            "broadcast: fps_calm と -maxrate を組み合わせると、平時の画面が崩れます (encode は CRF だけにしてください)"
        );
    }
    if cfg.record.is_some() && (cfg.encoder != EncoderKind::Ffmpeg || cfg.source != Source::Native) {
        tracing::warn!("broadcast: [record] は source = \"native\" かつ encoder = \"ffmpeg\" のときだけ働きます (今の設定では何もしません)");
    }
    if cfg
        .record
        .as_ref()
        .is_some_and(|r| r.before_min + r.after_min > record::MAX_SPAN_MIN)
    {
        tracing::warn!(
            "broadcast: before_min + after_min は {} 分までにしてください (ring は 1 分ごとの {} 個なので、古い分が欠けます)",
            record::MAX_SPAN_MIN,
            ring::RING_FILES
        );
    }
    let get = |k: &str| std::env::var(k).ok();
    let output = expand_all(&cfg.output, get)?;
    // 送り先に埋めた値 (ストリームキーなど) はログで伏せる
    let secrets = secrets_of(&cfg.output, get);
    // 止める合図 (Ctrl+C・SIGTERM) を受けたら、動かしている Chrome・ffmpeg・音のコマンドを止めてから終える
    // (session を途中で捨てると kill_on_drop で子のプロセスが止まる。止めないと ffmpeg が送り先につないだまま残る)
    let stop = crate::shutdown_signal();
    tokio::pin!(stop);
    loop {
        let mut wait = RESTART_AFTER;
        tokio::select! {
            r = session(&cfg, &output, &secrets) => match r {
                Ok(()) => tracing::warn!("broadcast: stopped"),
                Err(e) if e.is::<Refused>() => {
                    tracing::error!("broadcast: 配信を始めません: {e}");
                    wait = REFUSED_WAIT;
                }
                Err(e) => tracing::warn!("broadcast: {}", redact(&format!("{e:#}"), &secrets)),
            },
            _ = &mut stop => break,
        }
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = &mut stop => break,
        }
    }
    tracing::info!("broadcast: stopping");
    Ok(())
}

/// Chrome と ffmpeg を 1 組起動し、どちらかが止まるまで画面を送り続ける
async fn session(cfg: &BroadcastConfig, output: &[String], secrets: &[String]) -> anyhow::Result<()> {
    let (notice_tx, notice_rx) = tokio::sync::mpsc::unbounded_channel();
    let notices = cfg.mixer.then_some(notice_tx);
    // 最後にエンコーダへ送ったコマの時刻 (状態のファイルに書く)。前の値が残っていれば、新しいコマを送るまで保つ
    let last_frame = Arc::new(AtomicU64::new(0));
    let load = status::Load::new();
    let state_path = calm_state::path_of(&cfg.state_file);
    // 画面: chrome (JPEG を受け取る) か native (Rust で描いて I420 にした画面を watch で受け取る) のどちらか
    let (mut screen, mut chrome, mut native) = match cfg.source {
        Source::Chrome => {
            let page = if cfg.mixer {
                with_query(&cfg.url, "audio=mixer")
            } else {
                cfg.url.clone()
            };
            let (s, c) = chrome::launch(cfg, &page, notices).await?;
            (Some(s), Some(c), None)
        }
        Source::Native => {
            native::check_source(cfg).await?;
            // 組み込みの定義が使えない (起こらないはず): 繰り返しても直らないので、check_replay と同じく長く待つ
            let layouts = native::load_layouts(cfg)
                .await
                .map_err(|e| Refused(format!("レイアウトの定義を使えません: {e:#}")))?;
            // 前の最後のコマから 30 秒以上あいていれば、途切れた札を出す (docs/broadcast-status.md)
            let prev = calm_state::read(&state_path).and_then(|s| s.last_frame_ms);
            last_frame.store(prev.unwrap_or(0), Ordering::Relaxed);
            let outage = status::outage_of(prev, calm_state::now_ms());
            // 見張りは、描く係 (native) が消えると終わる
            let (busy, _) = status::spawn(load.clone());
            (
                None,
                None,
                Some(native::start(cfg, layouts, notices, status::Feed { busy, outage })?),
            )
        }
    };
    // mixer の音 (fifo は mixer より後に捨てる。宣言の順を変えないこと)
    let mixer_fifo = if cfg.mixer { Some(audio::Fifo::create()?) } else { None };
    let _mixer = mixer_fifo
        .as_ref()
        .map(|f| mixer::spawn(notice_rx, f.path().to_path_buf(), cfg.bgm_url.clone()));
    let mut audio_cmd = match cfg.audio_command.is_empty() || cfg.mixer {
        true => None,
        false => Some(audio::AudioCommand::start(&cfg.audio_command)?),
    };
    let audio_in = match (&mixer_fifo, &audio_cmd) {
        (Some(f), _) => f.ffmpeg_input(mixer::RATE),
        (None, Some(a)) => a.ffmpeg_input(),
        (None, None) => cfg.audio.clone(),
    };
    // [record] は native・ffmpeg のときだけ (run で警告する)
    let record = cfg
        .record
        .as_ref()
        .filter(|_| native.is_some() && cfg.encoder == EncoderKind::Ffmpeg);
    if let (Some(rc), Some(n)) = (record, &native) {
        record::spawn(rc.clone(), cfg.ffmpeg.clone(), n.calm.clone(), n.shown.clone())?;
    }
    // 地震の画面か平時かを、再現動画を作る係に知らせる (書けなくても配信は続ける)
    if let Some(n) = &native {
        calm_state::spawn(state_path, n.calm.clone(), n.frames.clone(), last_frame.clone());
    }
    let mut encoder = match cfg.encoder {
        EncoderKind::Ffmpeg => Encoder::Ffmpeg(Box::new(ffmpeg::FfmpegEncoder::start(
            cfg, &audio_in, output, secrets, record,
        )?)),
        EncoderKind::Builtin => Encoder::Builtin(Box::new(builtin::BuiltinEncoder::start(cfg, output).await?)),
    };
    let started = std::time::Instant::now();
    tracing::info!(source = ?cfg.source, url = %cfg.url, width = cfg.width, height = cfg.height, fps = cfg.fps, fps_calm = ?cfg.fps_calm, "broadcast started");
    let mut frame: Arc<Vec<u8>> = Arc::default();
    let mut rate = frame_rate(cfg, true);
    let mut tick = new_tick(rate);
    loop {
        tokio::select! {
            f = async { match screen.as_mut() { Some(s) => s.next_frame().await, None => std::future::pending().await } } => {
                let (data, session) = f?;
                if let Some(s) = screen.as_mut() {
                    s.ack(session).await?;
                }
                frame = Arc::new(chrome::decode_base64(&data)?);
            }
            // native: 描き直された画面、または平時と地震の切り替え (すぐコマの間隔を変える)
            r = async {
                match native.as_mut() {
                    Some(n) => tokio::select! { r = n.frames.changed() => r.map(|_| true), r = n.calm.changed() => r.map(|_| false) },
                    None => std::future::pending().await,
                }
            } => {
                let is_frame = r.context("native renderer stopped")?;
                if let Some(n) = native.as_mut() {
                    if is_frame {
                        frame = n.frames.borrow_and_update().clone();
                    } else {
                        let next = frame_rate(cfg, *n.calm.borrow_and_update());
                        if next != rate {
                            tracing::info!(fps = next, "broadcast: frame rate changed");
                            rate = next;
                            tick = new_tick(rate);
                        }
                    }
                }
            }
            // 変化が無くても同じ画面を送り続ける (fps_calm を指定していなければ、ffmpeg は枚数から時刻を決める)
            at = tick.tick(), if !frame.is_empty() => {
                // コマの予定の時刻からの遅れと、書き込みの長さを残す (混雑中の札の材料)
                let lag = at.elapsed();
                load.begin_write();
                let sent = encoder.video(&frame, started.elapsed().as_millis() as u64).await;
                load.end_write(lag);
                sent?;
                last_frame.store(calm_state::now_ms(), Ordering::Relaxed);
            }
            e = encoder.closed() => return Err(e),
            status = async { match chrome.as_mut() { Some(c) => c.wait().await, None => std::future::pending().await } } => {
                anyhow::bail!("chrome exited: {}", status?)
            }
            status = async {
                match audio_cmd.as_mut() {
                    Some(a) => a.child.wait().await,
                    None => std::future::pending().await,
                }
            } => anyhow::bail!("audio_command exited: {}", status?),
        }
    }
}

fn new_tick(fps: u32) -> tokio::time::Interval {
    let mut t = tokio::time::interval(Duration::from_secs_f64(1.0 / fps.max(1) as f64));
    t.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    t
}

/// 送るコマ数 (fps_calm があれば、平時はそれ)
fn frame_rate(cfg: &BroadcastConfig, calm: bool) -> u32 {
    match cfg.fps_calm {
        Some(f) if calm && cfg.source == Source::Native => f,
        _ => cfg.fps,
    }
}

/// URL に問い合わせの項目を足す (# の前に入れる)
fn with_query(url: &str, item: &str) -> String {
    let (base, frag) = url.split_once('#').map_or((url, ""), |(b, f)| (b, f));
    let sep = if base.contains('?') { '&' } else { '?' };
    let hash = if frag.is_empty() {
        String::new()
    } else {
        format!("#{frag}")
    };
    format!("{base}{sep}{item}{hash}")
}

fn ffmpeg_args(cfg: &BroadcastConfig, audio: &[String], output: &[String]) -> Vec<String> {
    let fps = cfg.fps.max(1).to_string();
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let mut a = s(&["-hide_banner", "-loglevel", "warning"]);
    // 可変 fps: 届いた時刻をコマの時刻にする (native で fps_calm を指定したときだけ)
    let variable = cfg.source == Source::Native && cfg.fps_calm.is_some();
    match cfg.source {
        Source::Chrome => a.extend(s(&["-f", "image2pipe", "-c:v", "mjpeg"])),
        Source::Native => {
            let size = format!("{}x{}", cfg.width, cfg.height);
            if variable {
                a.extend(s(&["-use_wallclock_as_timestamps", "1"]));
            }
            a.extend(s(&["-f", "rawvideo", "-pix_fmt", "yuv420p", "-s", &size]));
        }
    }
    if variable {
        a.extend(s(&["-i", "-"]));
    } else {
        a.extend(s(&["-framerate", &fps, "-i", "-"]));
    }
    if audio.is_empty() {
        // 無音も実時間の速さで作る (そうしないと音だけ先に進み、映像とずれる)
        a.extend(s(&["-re", "-f", "lavfi", "-i", "anullsrc=r=44100:cl=stereo"]));
    } else {
        a.extend(audio.iter().cloned());
    }
    a.extend(s(&["-map", "0:v", "-map", "1:a"]));
    // 画面の大きさをそろえる (Chrome の最初の画面は表示の大きさを決める前のもので、縦が足りないことがある)
    let (w, h) = (cfg.width, cfg.height);
    let fit = format!("scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1");
    // (native は 1280x720 で描くので、そろえる必要が無い)
    if cfg.source == Source::Chrome {
        a.extend(["-vf".to_string(), fit]);
    }
    // 可変 fps で encode を指定していなければ、上限の無い CRF にする (variable_fps_encode)
    let encode = if variable && cfg.encode == default_encode() {
        variable_fps_encode()
    } else {
        cfg.encode.clone()
    };
    a.extend(encode);
    // YouTube などはキーフレームの間隔を 4 秒以下に求める (2 秒ごとにする)
    if variable {
        // コマの間隔が一定でないので、コマ数ではなく時刻で決める
        a.extend(s(&[
            "-fps_mode",
            "passthrough",
            "-force_key_frames",
            "expr:gte(t,n_forced*2)",
        ]));
    } else {
        a.extend(["-g".to_string(), (cfg.fps.max(1) * 2).to_string()]);
    }
    a.extend(s(&["-pix_fmt", "yuv420p", "-c:a", "aac"]));
    a.extend(["-b:a".to_string(), cfg.audio_bitrate.clone()]);
    a.extend(s(&["-ar", "44100"]));
    a.extend(output.iter().cloned());
    a
}

/// `$VAR` / `${VAR}` を環境変数の値に置き換える (無ければエラー)
fn expand(s: &str, get: impl Fn(&str) -> Option<String>) -> anyhow::Result<String> {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('$') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let (name, next) = match after.strip_prefix('{') {
            Some(b) => {
                let end = b.find('}').context("unclosed ${")?;
                (&b[..end], &b[end + 1..])
            }
            None => {
                let end = after
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .unwrap_or(after.len());
                (&after[..end], &after[end..])
            }
        };
        anyhow::ensure!(!name.is_empty(), "empty variable name in output");
        out.push_str(&get(name).with_context(|| format!("environment variable {name} is not set"))?);
        rest = next;
    }
    out.push_str(rest);
    Ok(out)
}

fn expand_all(args: &[String], get: impl Fn(&str) -> Option<String>) -> anyhow::Result<Vec<String>> {
    args.iter().map(|a| expand(a, &get)).collect()
}

/// 送り先に埋める環境変数の値 (ログで伏せる)
fn secrets_of(args: &[String], get: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let names = args.iter().flat_map(|a| {
        a.split('$').skip(1).map(|v| {
            let v = v.strip_prefix('{').map_or(v, |b| b.split('}').next().unwrap_or(""));
            v.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .next()
                .unwrap_or("")
                .to_string()
        })
    });
    names.filter_map(|n| get(&n)).filter(|v| !v.is_empty()).collect()
}

fn redact(line: &str, secrets: &[String]) -> String {
    secrets
        .iter()
        .fold(line.to_string(), |l, s| l.replace(s.as_str(), "***"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(k: &str) -> Option<String> {
        (k == "KEY").then(|| "abc-123".to_string())
    }

    #[test]
    fn the_layout_names_default_to_the_broadcast_definitions_and_can_be_changed() {
        let cfg = toml::from_str::<BroadcastConfig>("source = \"native\"").unwrap();
        assert_eq!(
            (cfg.layout.as_str(), cfg.layout_quake.as_str()),
            ("broadcast", "broadcast-quake")
        );
        let cfg = toml::from_str::<BroadcastConfig>("layout = \"jdq\"\nlayout_quake = \"jdq-quake\"").unwrap();
        assert_eq!((cfg.layout.as_str(), cfg.layout_quake.as_str()), ("jdq", "jdq-quake"));
    }

    #[test]
    fn output_takes_the_stream_key_from_the_environment() {
        assert_eq!(expand("rtmps://x/live2/$KEY", env).unwrap(), "rtmps://x/live2/abc-123");
        assert_eq!(expand("a${KEY}b/$KEY.flv", env).unwrap(), "aabc-123b/abc-123.flv");
        assert_eq!(expand("plain", env).unwrap(), "plain");
        assert!(expand("$NOPE", env).is_err());
        assert!(expand("${KEY", env).is_err());
    }

    #[test]
    fn the_stream_key_is_hidden_in_logs() {
        let out = vec![
            "-f".to_string(),
            "flv".to_string(),
            "rtmps://x/live2/${KEY}".to_string(),
        ];
        let secrets = secrets_of(&out, env);
        assert_eq!(secrets, vec!["abc-123"]);
        assert_eq!(
            redact("rtmps://x/live2/abc-123: I/O error", &secrets),
            "rtmps://x/live2/***: I/O error"
        );
    }

    #[test]
    fn the_page_is_told_to_use_the_mixer() {
        assert_eq!(
            with_query("https://x/?broadcast=1", "audio=mixer"),
            "https://x/?broadcast=1&audio=mixer"
        );
        assert_eq!(with_query("https://x/", "audio=mixer"), "https://x/?audio=mixer");
        assert_eq!(
            with_query("https://x/?a=1#top", "audio=mixer"),
            "https://x/?a=1&audio=mixer#top"
        );
    }

    #[test]
    fn mixer_audio_is_read_as_s16le_44100_stereo() {
        let fifo = audio::Fifo::create().unwrap();
        let a = fifo.ffmpeg_input(mixer::RATE);
        assert_eq!(&a[..6], ["-f", "s16le", "-ar", "44100", "-ac", "2"]);
        assert_eq!(a[6], "-i");
        assert!(toml::from_str::<BroadcastConfig>("mixer = true").unwrap().mixer);
        assert!(!BroadcastConfig::default().mixer);
    }

    #[test]
    fn native_video_is_raw_yuv420p_and_chrome_stays_mjpeg() {
        let out = ["out.flv".to_string()];
        let chrome = ffmpeg_args(&BroadcastConfig::default(), &[], &out);
        assert!(chrome.windows(2).any(|w| w == ["-f", "image2pipe"]));
        assert!(chrome.windows(2).any(|w| w == ["-c:v", "mjpeg"]));
        assert!(chrome.contains(&"-vf".to_string()));
        assert!(!chrome.contains(&"rawvideo".to_string()));
        let cfg = toml::from_str::<BroadcastConfig>("source = \"native\"").unwrap();
        assert_eq!(cfg.source, Source::Native);
        let native = ffmpeg_args(&cfg, &[], &out);
        assert!(native.windows(2).any(|w| w == ["-f", "rawvideo"]));
        assert!(native
            .windows(4)
            .any(|w| w == ["-f", "rawvideo", "-pix_fmt", "yuv420p"]));
        assert!(!native.contains(&"rgba".to_string()));
        assert!(native.windows(2).any(|w| w == ["-s", "1280x720"]));
        assert!(!native.contains(&"mjpeg".to_string()));
        assert_eq!(BroadcastConfig::default().source, Source::Chrome);
    }

    #[test]
    fn silent_audio_is_used_without_an_audio_input() {
        let cfg = BroadcastConfig::default();
        let a = ffmpeg_args(&cfg, &[], &["out.flv".to_string()]);
        assert!(a.windows(2).any(|w| w == ["-i", "anullsrc=r=44100:cl=stereo"]));
        assert_eq!(a.last().unwrap(), "out.flv");
        assert!(a.windows(2).any(|w| w == ["-g", "60"]));
    }

    #[test]
    fn variable_fps_uses_wallclock_timestamps_and_time_based_keyframes() {
        let out = ["out.flv".to_string()];
        let cfg = toml::from_str::<BroadcastConfig>("source = \"native\"\nfps = 10\nfps_calm = 2").unwrap();
        let a = ffmpeg_args(&cfg, &[], &out);
        let pos = |k: &str| a.iter().position(|x| x == k).unwrap();
        assert_eq!(a[pos("-use_wallclock_as_timestamps") + 1], "1");
        // 入力の指定より前に置く (入力のオプション)
        assert!(pos("-use_wallclock_as_timestamps") < pos("-i"));
        assert!(!a.contains(&"-framerate".to_string()));
        assert!(a.windows(2).any(|w| w == ["-fps_mode", "passthrough"]));
        assert!(a
            .windows(2)
            .any(|w| w == ["-force_key_frames", "expr:gte(t,n_forced*2)"]));
        assert!(!a.contains(&"-g".to_string()));
        assert_eq!(frame_rate(&cfg, true), 2);
        assert_eq!(frame_rate(&cfg, false), 10);
    }

    #[test]
    fn variable_fps_default_encode_has_no_maxrate_but_an_explicit_one_is_kept() {
        let out = ["out.flv".to_string()];
        let cfg = toml::from_str::<BroadcastConfig>("source = \"native\"\nfps_calm = 2").unwrap();
        let a = ffmpeg_args(&cfg, &[], &out);
        assert!(!a.contains(&"-maxrate".to_string()) && a.windows(2).any(|w| w == ["-crf", "23"]));
        let cfg = toml::from_str::<BroadcastConfig>("source = \"native\"\nfps_calm = 2\nencode = [\"-crf\", \"30\"]")
            .unwrap();
        let a = ffmpeg_args(&cfg, &[], &out);
        assert!(a.windows(2).any(|w| w == ["-crf", "30"]) && !a.contains(&"-preset".to_string()));
        // 一定のコマ数では、今までの上限つきのまま
        let cfg = toml::from_str::<BroadcastConfig>("source = \"native\"").unwrap();
        assert!(ffmpeg_args(&cfg, &[], &out).contains(&"-maxrate".to_string()));
    }

    #[test]
    fn without_fps_calm_native_stays_constant_rate() {
        let out = ["out.flv".to_string()];
        let cfg = toml::from_str::<BroadcastConfig>("source = \"native\"\nfps = 5").unwrap();
        let a = ffmpeg_args(&cfg, &[], &out);
        assert!(a.windows(2).any(|w| w == ["-framerate", "5"]));
        assert!(a.windows(2).any(|w| w == ["-g", "10"]));
        assert!(!a.contains(&"-use_wallclock_as_timestamps".to_string()));
        assert!(!a.contains(&"-fps_mode".to_string()));
        assert_eq!(frame_rate(&cfg, true), 5);
    }

    #[test]
    fn chrome_ignores_fps_calm() {
        let out = ["out.flv".to_string()];
        let cfg = toml::from_str::<BroadcastConfig>("fps = 30\nfps_calm = 2").unwrap();
        let a = ffmpeg_args(&cfg, &[], &out);
        assert!(a.windows(2).any(|w| w == ["-framerate", "30"]));
        assert!(!a.contains(&"-use_wallclock_as_timestamps".to_string()));
        assert_eq!(frame_rate(&cfg, true), 30);
    }

    #[test]
    fn audio_bitrate_is_configurable() {
        let out = ["out.flv".to_string()];
        let a = ffmpeg_args(&BroadcastConfig::default(), &[], &out);
        assert!(a.windows(2).any(|w| w == ["-b:a", "128k"]));
        let cfg = toml::from_str::<BroadcastConfig>("audio_bitrate = \"32k\"").unwrap();
        assert!(ffmpeg_args(&cfg, &[], &out).windows(2).any(|w| w == ["-b:a", "32k"]));
    }
}
