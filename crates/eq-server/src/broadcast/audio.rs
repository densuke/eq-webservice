//! 音を名前付きパイプで ffmpeg に渡す。macOS の ffmpeg (avfoundation) は音のサンプルを 1 割ほど落とすため、
//! 音は ffmpeg の外で用意する。
//! - AudioCommand: CoreAudio から欠けずに録れるコマンド (sox など) の標準出力 (s16le・48kHz・ステレオ)
//! - mixer (mixer/): eq-server 自身が作った音 (s16le・44.1kHz・ステレオ)

use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::Context;
use tokio::process::{Child, Command};

/// audio_command が出す音の形式 (ffmpeg にもこの形式として読ませる)
pub const RATE: u32 = 48_000;

/// 名前付きパイプ。捨てると消える
pub struct Fifo {
    path: PathBuf,
}

impl Fifo {
    pub fn create() -> anyhow::Result<Fifo> {
        let path = std::env::temp_dir().join(format!("eq-broadcast-audio-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let made = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .context("mkfifo")?;
        anyhow::ensure!(made.success(), "mkfifo {} failed", path.display());
        Ok(Fifo { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// ffmpeg の入力の引数 (s16le・ステレオ)
    pub fn ffmpeg_input(&self, rate: u32) -> Vec<String> {
        ["-f", "s16le", "-ar", &rate.to_string(), "-ac", "2", "-i"]
            .iter()
            .map(|s| s.to_string())
            .chain([self.path.display().to_string()])
            .collect()
    }
}

impl Drop for Fifo {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub struct AudioCommand {
    pub child: Child,
    fifo: Fifo,
}

impl AudioCommand {
    /// コマンドを起動し、その出力を名前付きパイプへ流し始める
    pub fn start(cmd: &[String]) -> anyhow::Result<AudioCommand> {
        let (prog, args) = cmd.split_first().context("audio_command is empty")?;
        let fifo = Fifo::create()?;
        let mut child = Command::new(prog)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("starting {prog}"))?;
        let mut out = child.stdout.take().context("audio_command stdout")?;
        let path = fifo.path().to_path_buf();
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
        self.fifo.ffmpeg_input(RATE)
    }
}
