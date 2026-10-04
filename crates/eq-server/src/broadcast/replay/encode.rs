//! ffmpeg を 2 回使って mp4 にする。先に音だけを AAC にし (生の音声の一時ファイルは作らない)、映像と合わせる。
//! 配信 (broadcast/ffmpeg.rs) と違い、時刻は実時間でなくコマの数で決まる (`-re`・`-use_wallclock_as_timestamps`・`anullsrc` は使わない)。

use std::path::Path;
use std::process::Stdio;

use anyhow::Context;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};

use super::plan::{duration_ms, Frame, Plan};
use super::voice::{self, Clip};
use super::Options;
use crate::broadcast::mixer::{AudioMixer, Notice, RATE};
use crate::broadcast::native::{Icons, Input, Stepper};
use crate::broadcast::psi::Throttle;
use crate::quake::Event;

/// 音を混ぜる 1 回の長さ (20ms)。音を鳴らす時刻はこの単位に丸まる
const CHUNK_FRAMES: usize = RATE as usize / 50;
/// 画面の大きさ (native の描画の大きさ)
const SIZE: &str = "1280x720";

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// 音を AAC にする ffmpeg の引数。PCM (s16le・44.1kHz・ステレオ) を標準入力から受ける。
/// `-threads 1`: 付けないとメモリが増える (e2 は 1GB)
pub fn audio_args(audio: &Path) -> Vec<String> {
    let mut a = strings(&[
        "-hide_banner",
        "-loglevel",
        "warning",
        "-y",
        "-f",
        "s16le",
        "-ar",
        "44100",
        "-ac",
        "2",
        "-i",
        "-",
        "-threads",
        "1",
        "-c:a",
        "aac",
        "-b:a",
        "96k",
    ]);
    a.push(audio.display().to_string());
    a
}

/// 映像 (rawvideo の I420 を標準入力から受ける) と音のファイルを合わせて mp4 にする ffmpeg の引数。
/// コマは固定の fps で、時刻はコマの順番で付く。キーフレームは 2 秒ごと。
/// `-threads 1` と `-tune zerolatency` (先読みと B フレームをやめる) はメモリのため (e2 は 1GB。先読みありは約 130MB、なしで配信の ffmpeg と同じ程度)
pub fn video_args(fps: u32, audio: &Path, out: &Path) -> Vec<String> {
    let mut a = strings(&[
        "-hide_banner",
        "-loglevel",
        "warning",
        "-y",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "yuv420p",
        "-s",
        SIZE,
    ]);
    a.extend([
        "-framerate".to_string(),
        fps.to_string(),
        "-i".to_string(),
        "-".to_string(),
        "-i".to_string(),
    ]);
    a.push(audio.display().to_string());
    a.extend(strings(&[
        "-map",
        "0:v",
        "-map",
        "1:a",
        "-c:v",
        "libx264",
        "-preset",
        "veryfast",
        "-tune",
        "zerolatency",
        "-crf",
        "23",
        "-threads",
        "1",
        "-pix_fmt",
        "yuv420p",
    ]));
    a.extend(["-g".to_string(), (fps * 2).to_string()]);
    a.extend(strings(&["-c:a", "copy", "-shortest", "-movflags", "+faststart"]));
    a.push(out.display().to_string());
    a
}

fn spawn(ffmpeg: &str, args: Vec<String>) -> anyhow::Result<Child> {
    Command::new(ffmpeg)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("starting {ffmpeg}"))
}

/// 標準入力を閉じて、ffmpeg の終わりを待つ
async fn finish(mut child: Child, name: &str) -> anyhow::Result<()> {
    drop(child.stdin.take());
    let status = child.wait().await?;
    anyhow::ensure!(status.success(), "{name} が失敗しました ({status})");
    Ok(())
}

/// 筋書きの音 (警戒音・揺れの始まり・刻み) と読み上げの声を混ぜて、AAC のファイルにする
pub async fn encode_audio(
    o: &Options,
    plan: &Plan,
    voices: &[Clip],
    audio: &Path,
    throttle: &mut Throttle,
) -> anyhow::Result<()> {
    let mut child = spawn(&o.ffmpeg, audio_args(audio))?;
    let mut stdin = child.stdin.take().context("ffmpeg stdin")?;
    let mut mixer = AudioMixer::new(|| None);
    let total = duration_ms(plan.frames.len(), o.fps) * u64::from(RATE) / 1000;
    let mut sounds = plan.sounds.iter().peekable();
    let mut clips = voices.iter().peekable();
    let mut done = 0u64;
    while done < total {
        throttle.wait().await?;
        let at_ms = done * 1000 / u64::from(RATE);
        while let Some(s) = sounds.next_if(|s| s.video_ms <= at_ms) {
            mixer.apply(Notice::Alert { level: s.level });
        }
        voice::push_due(&mut mixer, &mut clips, at_ms);
        let n = CHUNK_FRAMES.min((total - done) as usize);
        let bytes: Vec<u8> = mixer.render(n).into_iter().flat_map(i16::to_le_bytes).collect();
        stdin.write_all(&bytes).await.context("writing audio to ffmpeg")?;
        done += n as u64;
    }
    drop(stdin);
    finish(child, "ffmpeg (音)").await
}

/// 1 コマの描画の入力。配信の状態の札 (混雑中・途切れ) は、手元で描き直す動画には出さないので、必ず None
fn frame_input<'a>(label: &'a str, events: &'a [Event], f: &'a Frame, icons: &'a Icons, check_ms: u64) -> Input<'a> {
    Input {
        events: &events[..f.applied],
        now: f.now_ms,
        rev: f.applied as u64,
        warnings: None,
        weather: None,
        icons,
        bgm_title: "",
        connected: true,
        label,
        test: false,
        check_ms,
        flip_s: 0,
        hindsight: f.hindsight.as_ref(),
        fast_forward: f.fast_forward,
        status: None,
        notices: None,
    }
}

/// 筋書きのコマを順に描いて、音のファイルと合わせて mp4 にする
pub async fn encode_video(
    o: &Options,
    events: &[Event],
    plan: &Plan,
    stepper: &mut Stepper,
    audio: &Path,
    throttle: &mut Throttle,
) -> anyhow::Result<()> {
    let mut child = spawn(&o.ffmpeg, video_args(o.fps, audio, &o.out))?;
    let mut stdin = child.stdin.take().context("ffmpeg stdin")?;
    let icons = Icons::new();
    let check_ms = 1000 / u64::from(o.fps);
    let mut last: Vec<u8> = Vec::new();
    for (i, f) in plan.frames.iter().enumerate() {
        throttle.wait().await?;
        let input = frame_input(&o.label, events, f, &icons, check_ms);
        // 前のコマと同じなら、描き直さずに同じ画面を渡す
        if let Some(out) = stepper.step(&input) {
            last = out.i420;
        }
        stdin.write_all(&last).await.context("writing a frame to ffmpeg")?;
        if i % 500 == 499 {
            tracing::info!("replay-video: {}/{} コマ", i + 1, plan.frames.len());
        }
    }
    drop(stdin);
    finish(child, "ffmpeg (映像)").await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has(a: &[String], pair: [&str; 2]) -> bool {
        a.windows(2).any(|w| w == pair)
    }

    #[test]
    fn the_video_args_use_frame_counts_not_the_wall_clock() {
        let a = video_args(5, Path::new("a.m4a"), Path::new("x.mp4"));
        for banned in ["-re", "-use_wallclock_as_timestamps", "anullsrc", "lavfi", "-fps_mode"] {
            assert!(!a.iter().any(|x| x.contains(banned)), "{banned} is in {a:?}");
        }
        assert!(has(&a, ["-framerate", "5"]));
        assert!(has(&a, ["-threads", "1"]));
        // 先読みをしない (e2 のメモリのため)
        assert!(has(&a, ["-tune", "zerolatency"]));
        assert!(has(&a, ["-movflags", "+faststart"]));
        assert!(has(&a, ["-c:v", "libx264"]) && has(&a, ["-crf", "23"]) && has(&a, ["-preset", "veryfast"]));
        assert!(has(&a, ["-c:a", "copy"]));
        assert_eq!(a.last().unwrap(), "x.mp4");
        // rawvideo の入力が先、音のファイルがあと
        let (v, s) = (
            a.iter().position(|x| x == "-").unwrap(),
            a.iter().position(|x| x == "a.m4a").unwrap(),
        );
        assert!(v < s);
    }

    #[test]
    fn replay_frames_never_carry_a_status_chip() {
        let f = Frame {
            now_ms: 1,
            applied: 0,
            fast_forward: false,
            hindsight: None,
        };
        let icons = Icons::new();
        let input = frame_input("記録から再現", &[], &f, &icons, 200);
        assert_eq!(input.status, None);
        assert_eq!(input.notices, None);
    }

    #[test]
    fn the_audio_args_read_pcm_from_stdin_and_write_aac() {
        let a = audio_args(Path::new("a.m4a"));
        assert!(has(&a, ["-f", "s16le"]) && has(&a, ["-ar", "44100"]) && has(&a, ["-ac", "2"]));
        assert!(has(&a, ["-c:a", "aac"]) && has(&a, ["-threads", "1"]));
        assert!(!a.iter().any(|x| x == "-re" || x.contains("anullsrc")));
        assert_eq!(a.last().unwrap(), "a.m4a");
    }
}
