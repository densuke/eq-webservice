//! ffmpeg を子のプロセスにして、画面を標準入力に渡す圧縮・送り出し (encoder = "ffmpeg"、既定)。
//! `[record]` があるときは、圧縮する ffmpeg の出力 (mpegts) を eq-server が受け、送り出し用の ffmpeg (`-c copy`) と
//! ring のファイルに分ける (docs/quake-archive.md 3.6 章)。録画が詰まっても失敗しても、送り出しは止めない。

use std::process::Stdio;

use anyhow::Context;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStderr, ChildStdin, Command};
use tokio::task::JoinHandle;

use super::{record::RecordConfig, redact, ring, BroadcastConfig};

/// 圧縮する ffmpeg から eq-server へ、mpegts を標準出力で渡す (`[record]` のとき、output の代わり)
const MPEGTS_PIPE: [&str; 3] = ["-f", "mpegts", "pipe:1"];

/// 子の ffmpeg と、その出力 (標準エラー)
struct Proc {
    child: Child,
    log: Lines<BufReader<ChildStderr>>,
    name: &'static str,
}

pub struct FfmpegEncoder {
    main: Proc,
    stdin: ChildStdin,
    /// `[record]` のとき、送り出し用の ffmpeg と、mpegts を分けるタスク
    sender: Option<(Proc, JoinHandle<()>)>,
    secrets: Vec<String>,
}

impl Drop for FfmpegEncoder {
    fn drop(&mut self) {
        if let Some((_, pump)) = &self.sender {
            pump.abort();
        }
    }
}

fn spawn(
    cfg: &BroadcastConfig,
    name: &'static str,
    args: Vec<String>,
    stdin: Stdio,
    stdout: Stdio,
) -> anyhow::Result<Proc> {
    let mut child = Command::new(&cfg.ffmpeg)
        .args(args)
        .stdin(stdin)
        .stdout(stdout)
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("starting {}", cfg.ffmpeg))?;
    let log = BufReader::new(child.stderr.take().context("ffmpeg stderr")?).lines();
    Ok(Proc { child, log, name })
}

/// 圧縮した mpegts を、送り出し用の ffmpeg の入力と ring に分ける。
/// 送り出しは待って渡す (今の直送と同じ詰まり方)。ring へは待たずに渡し、詰まったら捨てる
async fn pump(
    mut from: impl AsyncRead + Unpin,
    mut to: impl AsyncWrite + Unpin,
    ring: tokio::sync::mpsc::Sender<Vec<u8>>,
) {
    let mut buf = vec![0u8; 64 * 1024];
    while let Ok(n @ 1..) = from.read(&mut buf).await {
        let _ = ring.try_send(buf[..n].to_vec());
        if to.write_all(&buf[..n]).await.is_err() {
            return; // 送り出しが止まった。理由は sender の出力に出る
        }
    }
}

impl FfmpegEncoder {
    pub fn start(
        cfg: &BroadcastConfig,
        audio: &[String],
        output: &[String],
        secrets: &[String],
        record: Option<&RecordConfig>,
    ) -> anyhow::Result<Self> {
        let Some(rc) = record else {
            let mut main = spawn(
                cfg,
                "ffmpeg",
                super::ffmpeg_args(cfg, audio, output),
                Stdio::piped(),
                Stdio::null(),
            )?;
            let stdin = main.child.stdin.take().context("ffmpeg stdin")?;
            return Ok(Self {
                main,
                stdin,
                sender: None,
                secrets: secrets.to_vec(),
            });
        };
        let pipe = MPEGTS_PIPE.map(String::from);
        let mut main = spawn(
            cfg,
            "ffmpeg",
            super::ffmpeg_args(cfg, audio, &pipe),
            Stdio::piped(),
            Stdio::piped(),
        )?;
        let stdin = main.child.stdin.take().context("ffmpeg stdin")?;
        let from = main.child.stdout.take().context("ffmpeg stdout")?;
        // 圧縮し直さない。読み取りを短くして、送り出しの始まりを早める
        let mut args: Vec<String> = [
            "-hide_banner",
            "-loglevel",
            "warning",
            "-analyzeduration",
            "3000000",
            "-f",
            "mpegts",
            "-i",
            "pipe:0",
            "-c",
            "copy",
        ]
        .map(String::from)
        .to_vec();
        args.extend(output.iter().cloned());
        let mut send = spawn(cfg, "ffmpeg (送り出し)", args, Stdio::piped(), Stdio::null())?;
        let to = send.child.stdin.take().context("sender stdin")?;
        let pump = tokio::spawn(pump(from, to, ring::spawn(rc.ring_dir.clone().into())));
        Ok(Self {
            main,
            stdin,
            sender: Some((send, pump)),
            secrets: secrets.to_vec(),
        })
    }

    /// 1 コマ渡す。時刻は ffmpeg が決める (一定の fps なら枚数、可変 fps なら届いた時刻)
    pub async fn video(&mut self, frame: &[u8]) -> anyhow::Result<()> {
        if self.stdin.write_all(frame).await.is_err() {
            // ffmpeg が止まった。理由は ffmpeg の出力に出ているので、残りを読んでから終える
            return Err(self.closed().await);
        }
        Ok(())
    }

    /// ffmpeg (送り出しもあれば両方) の出力を流し続け、どれかが終わったら理由を返す
    pub async fn closed(&mut self) -> anyhow::Error {
        let secrets = &self.secrets;
        match self.sender.as_mut() {
            Some((send, _)) => {
                tokio::select! { e = self.main.closed(secrets) => e, e = send.closed(secrets) => e }
            }
            None => self.main.closed(secrets).await,
        }
    }
}

impl Proc {
    /// 出力を流し続け、終わったら理由を返す
    async fn closed(&mut self, secrets: &[String]) -> anyhow::Error {
        loop {
            match self.log.next_line().await {
                Ok(Some(l)) => tracing::warn!("{}: {}", self.name, redact(&l, secrets)),
                Ok(None) => break,
                Err(e) => return e.into(),
            }
        }
        match self.child.wait().await {
            Ok(s) => anyhow::anyhow!("{} exited: {s}", self.name),
            Err(e) => e.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_stuck_ring_does_not_hold_back_the_sender() {
        // ring の口 (容量 1) を誰も読まなくても、送り出しには全部が届く
        let (ring, _held) = tokio::sync::mpsc::channel(1);
        let (mut src, from) = tokio::io::duplex(1024);
        let (to, mut sink) = tokio::io::duplex(1 << 20);
        let task = tokio::spawn(pump(from, to, ring));
        let data = vec![7u8; 300_000];
        src.write_all(&data).await.unwrap();
        drop(src);
        task.await.unwrap();
        let mut got = Vec::new();
        sink.read_to_end(&mut got).await.unwrap();
        assert_eq!(got, data);
    }

    #[tokio::test]
    async fn the_pump_ends_when_the_sender_stops() {
        let (ring, _held) = tokio::sync::mpsc::channel(8);
        let (mut src, from) = tokio::io::duplex(1024);
        let (to, sink) = tokio::io::duplex(16);
        drop(sink); // 送り出しが止まった
        let task = tokio::spawn(pump(from, to, ring));
        src.write_all(&[1; 100]).await.unwrap();
        task.await.unwrap();
    }
}
