//! ライブ配信 (`eq-server broadcast <broadcast.toml>`)。画面の無い Chrome で地図のページを開き、
//! 画面の変化を受け取って (chrome.rs)、決まった fps で ffmpeg に渡して配信する。
//! 音は ffmpeg の入力で取り込む (Mac は BlackHole、Linux は PulseAudio のモニタなど)。
//! Chrome か ffmpeg が止まったら、両方を止めて少し待ってから立ち上げ直す。
//! 送り先 (ストリームキー入りの URL) は `$VAR` で環境変数から読み、ログには出さない。

mod audio;
mod chrome;
mod mixer;

use std::collections::BTreeMap;
use std::process::Stdio;
use std::time::Duration;

use anyhow::Context;
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct BroadcastConfig {
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
    /// 映像・音の圧縮 (ffmpeg の引数)
    pub encode: Vec<String>,
    /// 送り先 (ffmpeg の引数)。`$VAR` / `${VAR}` は環境変数に置き換える
    pub output: Vec<String>,
}

impl Default for BroadcastConfig {
    fn default() -> Self {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect();
        BroadcastConfig {
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
    let (mut screen, mut chrome) = chrome::launch(cfg).await?;
    let mut audio_cmd = match cfg.audio_command.is_empty() {
        true => None,
        false => Some(audio::AudioCommand::start(&cfg.audio_command)?),
    };
    let audio_in = audio_cmd
        .as_ref()
        .map_or_else(|| cfg.audio.clone(), |a| a.ffmpeg_input());
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
    tracing::info!(url = %cfg.url, width = cfg.width, height = cfg.height, fps = cfg.fps, "broadcast started");
    let mut frame: Vec<u8> = Vec::new();
    let mut tick = tokio::time::interval(Duration::from_secs_f64(1.0 / cfg.fps.max(1) as f64));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            f = screen.next_frame() => {
                let (data, session) = f?;
                screen.ack(session).await?;
                frame = chrome::decode_base64(&data)?;
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
            status = chrome.wait() => anyhow::bail!("chrome exited: {}", status?),
            status = async {
                match audio_cmd.as_mut() {
                    Some(a) => a.child.wait().await,
                    None => std::future::pending().await,
                }
            } => anyhow::bail!("audio_command exited: {}", status?),
        }
    }
}

fn ffmpeg_args(cfg: &BroadcastConfig, audio: &[String], output: &[String]) -> Vec<String> {
    let fps = cfg.fps.max(1).to_string();
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let mut a = s(&[
        "-hide_banner",
        "-loglevel",
        "warning",
        "-f",
        "image2pipe",
        "-c:v",
        "mjpeg",
    ]);
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
    a.extend(["-vf".to_string(), fit]);
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
    fn silent_audio_is_used_without_an_audio_input() {
        let cfg = BroadcastConfig::default();
        let a = ffmpeg_args(&cfg, &[], &["out.flv".to_string()]);
        assert!(a.windows(2).any(|w| w == ["-i", "anullsrc=r=44100:cl=stereo"]));
        assert_eq!(a.last().unwrap(), "out.flv");
        assert!(a.windows(2).any(|w| w == ["-g", "60"]));
    }
}
