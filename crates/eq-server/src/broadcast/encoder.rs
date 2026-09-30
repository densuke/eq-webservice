//! 圧縮して送るもののつなぎ目 (docs/broadcast-builtin.md 3 章)。
//! コマを送る間隔 (可変 fps) は Broadcaster が決め、Encoder は渡されたコマと時刻を使うだけ。

use super::builtin::BuiltinEncoder;
use super::ffmpeg::FfmpegEncoder;

pub enum Encoder {
    /// ffmpeg の子のプロセスに rawvideo (Chrome は JPEG) を渡す (既定)
    Ffmpeg(Box<FfmpegEncoder>),
    /// eq-server の中で H.264・AAC・FLV にして RTMP で送る (実験。native・無音のみ)
    Builtin(Box<BuiltinEncoder>),
}

impl Encoder {
    /// 1 コマ送る。frame は native なら I420、Chrome なら JPEG。pts_ms は配信の開始からのミリ秒 (単調に増える)
    pub async fn video(&mut self, frame: &[u8], pts_ms: u64) -> anyhow::Result<()> {
        match self {
            Encoder::Ffmpeg(e) => e.video(frame).await,
            Encoder::Builtin(e) => e.video(frame, pts_ms).await,
        }
    }

    /// 止まった理由 (送り先が切れた、ffmpeg が終わった)。止まるまで返らない
    pub async fn closed(&mut self) -> anyhow::Error {
        match self {
            Encoder::Ffmpeg(e) => e.closed().await,
            Encoder::Builtin(e) => e.closed().await,
        }
    }
}
