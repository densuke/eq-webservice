//! ライブ配信 (`eq-server broadcast <broadcast.toml>`)。画面の無い Chrome で地図のページを開き、
//! 画面の変化を受け取って (chrome.rs)、決まった fps で ffmpeg に渡して配信する。
//! 音は mixer (eq-server 自身が BGM と警戒音を混ぜる) か、ffmpeg の入力 (Mac は BlackHole、Linux は PulseAudio のモニタなど) で取り込む。
//! Chrome か ffmpeg が止まったら、両方を止めて少し待ってから立ち上げ直す。
//! 送り先 (ストリームキー入りの URL) は `$VAR` で環境変数から読み、ログには出さない。

mod audio;
mod chrome;
mod mixer;
mod native;

use std::collections::BTreeMap;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

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
    pub fps: u32,
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
    /// mixer が流す BGM (Icecast の MP3)。空なら BGM は流さない
    pub bgm_url: String,
    /// 映像・音の圧縮 (ffmpeg の引数)
    pub encode: Vec<String>,
    /// 送り先 (ffmpeg の引数)。`$VAR` / `${VAR}` は環境変数に置き換える
    pub output: Vec<String>,
}

impl Default for BroadcastConfig {
    fn default() -> Self {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect();
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
            bgm_url: "https://eq.fuga.jp/stream/bgm.mp3".into(),
            encode: s(&[
                "-c:v", "libx264", "-preset", "veryfast", "-b:v", "3000k", "-maxrate", "3000k", "-bufsize", "6000k",
            ]),
            output: Vec::new(),
        }
    }
}

/// 止まってから立ち上げ直すまで
const RESTART_AFTER: Duration = Duration::from_secs(5);

pub async fn run(args: &[String]) -> anyhow::Result<()> {
    let [path] = args else {
        anyhow::bail!("usage: eq-server broadcast <broadcast.toml>");
    };
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
    let cfg: BroadcastConfig = toml::from_str(&text).with_context(|| format!("parsing {path}"))?;
    anyhow::ensure!(!cfg.output.is_empty(), "output (送り先) を設定してください");
    let get = |k: &str| std::env::var(k).ok();
    let output = expand_all(&cfg.output, get)?;
    // 送り先に埋めた値 (ストリームキーなど) はログで伏せる
    let secrets = secrets_of(&cfg.output, get);
    // 止める合図 (Ctrl+C・SIGTERM) を受けたら、動かしている Chrome・ffmpeg・音のコマンドを止めてから終える
    // (session を途中で捨てると kill_on_drop で子のプロセスが止まる。止めないと ffmpeg が送り先につないだまま残る)
    let stop = crate::shutdown_signal();
    tokio::pin!(stop);
    loop {
        tokio::select! {
            r = session(&cfg, &output, &secrets) => match r {
                Ok(()) => tracing::warn!("broadcast: stopped"),
                Err(e) => tracing::warn!("broadcast: {}", redact(&format!("{e:#}"), &secrets)),
            },
            _ = &mut stop => break,
        }
        tokio::select! {
            _ = tokio::time::sleep(RESTART_AFTER) => {}
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
        Source::Native => (None, None, Some(native::start(cfg, notices)?)),
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
    let mut ffmpeg = Command::new(&cfg.ffmpeg)
        .args(ffmpeg_args(cfg, &audio_in, output))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("starting {}", cfg.ffmpeg))?;
    let mut stdin = ffmpeg.stdin.take().context("ffmpeg stdin")?;
    let mut log = BufReader::new(ffmpeg.stderr.take().context("ffmpeg stderr")?).lines();
    tracing::info!(source = ?cfg.source, url = %cfg.url, width = cfg.width, height = cfg.height, fps = cfg.fps, "broadcast started");
    let mut frame: Arc<Vec<u8>> = Arc::default();
    let mut tick = tokio::time::interval(Duration::from_secs_f64(1.0 / cfg.fps.max(1) as f64));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            f = async { match screen.as_mut() { Some(s) => s.next_frame().await, None => std::future::pending().await } } => {
                let (data, session) = f?;
                if let Some(s) = screen.as_mut() {
                    s.ack(session).await?;
                }
                frame = Arc::new(chrome::decode_base64(&data)?);
            }
            // native: 描き直された画面
            r = async { match native.as_mut() { Some(n) => n.frames.changed().await, None => std::future::pending().await } } => {
                r.context("native renderer stopped")?;
                if let Some(n) = native.as_mut() {
                    frame = n.frames.borrow_and_update().clone();
                }
            }
            // 変化が無くても同じ画面を送り続け、fps を一定にする (ffmpeg は枚数から時刻を決める)
            _ = tick.tick(), if !frame.is_empty() => {
                if stdin.write_all(&frame).await.is_err() {
                    // ffmpeg が止まった。理由は ffmpeg の出力に出ているので、残りを読んでから終える
                    while let Ok(Some(l)) = log.next_line().await {
                        tracing::warn!("ffmpeg: {}", redact(&l, secrets));
                    }
                    anyhow::bail!("ffmpeg exited: {}", ffmpeg.wait().await?);
                }
            }
            line = log.next_line() => match line? {
                Some(l) => tracing::warn!("ffmpeg: {}", redact(&l, secrets)),
                None => anyhow::bail!("ffmpeg exited: {}", ffmpeg.wait().await?),
            },
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
    match cfg.source {
        Source::Chrome => a.extend(s(&["-f", "image2pipe", "-c:v", "mjpeg"])),
        Source::Native => {
            let size = format!("{}x{}", cfg.width, cfg.height);
            a.extend(s(&["-f", "rawvideo", "-pix_fmt", "yuv420p", "-s", &size]));
        }
    }
    a.extend(s(&["-framerate", &fps, "-i", "-"]));
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
    a.extend(cfg.encode.iter().cloned());
    // YouTube などはキーフレームの間隔を 4 秒以下に求める (2 秒ごとにする)
    let gop = (cfg.fps.max(1) * 2).to_string();
    a.extend(s(&[
        "-pix_fmt", "yuv420p", "-g", &gop, "-c:a", "aac", "-b:a", "128k", "-ar", "44100",
    ]));
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
}
