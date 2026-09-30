//! ffmpeg を子のプロセスにして、画面を標準入力に渡す圧縮・送り出し (encoder = "ffmpeg"、既定)。

use std::process::Stdio;

use anyhow::Context;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStderr, ChildStdin, Command};

use super::{redact, BroadcastConfig};

pub struct FfmpegEncoder {
    child: Child,
    stdin: ChildStdin,
    log: Lines<BufReader<ChildStderr>>,
    secrets: Vec<String>,
}

impl FfmpegEncoder {
    pub fn start(
        cfg: &BroadcastConfig,
        audio: &[String],
        output: &[String],
        secrets: &[String],
    ) -> anyhow::Result<Self> {
        let mut child = Command::new(&cfg.ffmpeg)
            .args(super::ffmpeg_args(cfg, audio, output))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("starting {}", cfg.ffmpeg))?;
        let stdin = child.stdin.take().context("ffmpeg stdin")?;
        let log = BufReader::new(child.stderr.take().context("ffmpeg stderr")?).lines();
        Ok(Self {
            child,
            stdin,
            log,
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

    /// ffmpeg の出力を流し続け、終わったら理由を返す
    pub async fn closed(&mut self) -> anyhow::Error {
        loop {
            match self.log.next_line().await {
                Ok(Some(l)) => tracing::warn!("ffmpeg: {}", redact(&l, &self.secrets)),
                Ok(None) => break,
                Err(e) => return e.into(),
            }
        }
        match self.child.wait().await {
            Ok(s) => anyhow::anyhow!("ffmpeg exited: {s}"),
            Err(e) => e.into(),
        }
    }
}
