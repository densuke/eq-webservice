//! 音を別のコマンド (sox など) で取り込む。macOS の ffmpeg (avfoundation) は音のサンプルを 1 割ほど落とすため、
//! CoreAudio から欠けずに録れるコマンドの標準出力 (s16le・48kHz・ステレオ) を名前付きパイプで ffmpeg に渡す。

use std::path::PathBuf;
use std::process::Stdio;

use anyhow::Context;
use tokio::process::{Child, Command};

/// audio_command が出す音の形式 (ffmpeg にもこの形式として読ませる)
pub const RATE: &str = "48000";

pub struct AudioCommand {
    pub child: Child,
    fifo: PathBuf,
}

impl AudioCommand {
    /// コマンドを起動し、その出力を名前付きパイプへ流し始める
    pub fn start(cmd: &[String]) -> anyhow::Result<AudioCommand> {
        let (prog, args) = cmd.split_first().context("audio_command is empty")?;
        let fifo = std::env::temp_dir().join(format!("eq-broadcast-audio-{}", std::process::id()));
        let _ = std::fs::remove_file(&fifo);
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .context("mkfifo")?;
        anyhow::ensure!(made.success(), "mkfifo {} failed", fifo.display());
        let mut child = Command::new(prog)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("starting {prog}"))?;
        let mut out = child.stdout.take().context("audio_command stdout")?;
        let path = fifo.clone();
        // 書き込み側を開くと ffmpeg が読み始めるまで待つので、別のタスクで流す
        tokio::spawn(async move {
            if let Ok(mut f) = tokio::fs::OpenOptions::new().write(true).open(&path).await {
                let _ = tokio::io::copy(&mut out, &mut f).await;
            }
        });
        Ok(AudioCommand { child, fifo })
    }

    /// ffmpeg の入力の引数
    pub fn ffmpeg_input(&self) -> Vec<String> {
        ["-f", "s16le", "-ar", RATE, "-ac", "2", "-i"]
            .iter()
            .map(|s| s.to_string())
            .chain([self.fifo.display().to_string()])
            .collect()
    }
}

impl Drop for AudioCommand {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.fifo);
    }
}
