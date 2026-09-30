//! eq-server の中だけで圧縮して送る (encoder = "builtin"、docs/broadcast-builtin.md)。
//! 映像は openh264 で H.264、音は無音の AAC、FLV に詰めて RTMP / RTMPS で送る (ファイルにも書ける)。
//! native の画面・無音のときだけ使える (音のある配信は ffmpeg を使う)。

mod aac;
mod amf;
mod chunk;
mod flv;
mod h264;
mod rtmp;
#[cfg(test)]
mod tests;

use anyhow::Context;
use tokio::fs::File;
use tokio::io::{AsyncWriteExt, BufWriter};

use super::BroadcastConfig;

/// 送り先: RTMP か、確かめる用のファイル (FLV)
enum Sink {
    Rtmp(rtmp::Client),
    File(BufWriter<File>),
}

impl Sink {
    async fn open(target: &str) -> anyhow::Result<Self> {
        if target.starts_with("rtmp://") || target.starts_with("rtmps://") {
            return Ok(Sink::Rtmp(rtmp::Client::connect(target).await?));
        }
        let mut f = BufWriter::new(
            File::create(target)
                .await
                .with_context(|| format!("{target} を作れません"))?,
        );
        f.write_all(&flv::header()).await?;
        Ok(Sink::File(f))
    }

    async fn send(&mut self, kind: u8, ts: u32, body: &[u8]) -> anyhow::Result<()> {
        match self {
            Sink::Rtmp(c) => c.send(kind, ts, body).await,
            Sink::File(f) => Ok(f.write_all(&flv::tag(kind, ts, body)).await?),
        }
    }

    async fn flush(&mut self) -> anyhow::Result<()> {
        match self {
            Sink::Rtmp(_) => Ok(()),
            Sink::File(f) => Ok(f.flush().await?),
        }
    }
}

pub struct BuiltinEncoder {
    h264: h264::H264,
    sink: Sink,
    silence: aac::Silence,
    /// 送り済みの SPS・PPS (変わったら送り直す)
    sent_config: Option<(Vec<u8>, Vec<u8>)>,
}

impl BuiltinEncoder {
    /// 設定が builtin で使えるものか (native・無音・送り先あり)
    pub fn check(cfg: &BroadcastConfig) -> anyhow::Result<()> {
        anyhow::ensure!(
            cfg.source == super::Source::Native,
            r#"encoder = "builtin" は source = "native" のときだけ使えます"#
        );
        anyhow::ensure!(
            !cfg.mixer && cfg.audio.is_empty() && cfg.audio_command.is_empty(),
            r#"encoder = "builtin" は無音の配信だけです (音のある配信は encoder = "ffmpeg")"#
        );
        Ok(())
    }

    pub async fn start(cfg: &BroadcastConfig, output: &[String]) -> anyhow::Result<Self> {
        let target = output.first().context("output (送り先) がありません")?;
        let h264 = h264::H264::new(cfg.width, cfg.height, cfg.builtin_bitrate, cfg.fps.max(1))?;
        let mut sink = Sink::open(target).await?;
        let rtmp = matches!(sink, Sink::Rtmp(_));
        let meta = flv::metadata_body(cfg.width, cfg.height, cfg.fps, cfg.builtin_bitrate / 1000, rtmp);
        sink.send(flv::TAG_DATA, 0, &meta).await?;
        sink.send(flv::TAG_AUDIO, 0, &flv::audio_config_body(&aac::AUDIO_SPECIFIC_CONFIG))
            .await?;
        Ok(Self {
            h264,
            sink,
            silence: aac::Silence::default(),
            sent_config: None,
        })
    }

    /// 1 コマ (I420) を圧縮して送る。その時刻までの音のコマも、時刻順に先に出す
    pub async fn video(&mut self, i420: &[u8], pts_ms: u64) -> anyhow::Result<()> {
        let enc = self.h264.encode(i420, pts_ms)?;
        let nals = flv::split_annex_b(&enc.annex_b);
        // ponytail: FLV の時刻は 32 ビット (約 49 日でひと回り)。それより長く続けるなら、つなぎ直す
        let ts = pts_ms as u32;
        let param = |t| nals.iter().find(|n| flv::nal_type(n) == t).map(|n| n.to_vec());
        if let (Some(sps), Some(pps)) = (param(7), param(8)) {
            if self.sent_config.as_ref() != Some(&(sps.clone(), pps.clone())) {
                self.sink
                    .send(flv::TAG_VIDEO, ts, &flv::video_config_body(&sps, &pps))
                    .await?;
                self.sent_config = Some((sps, pps));
            }
        }
        for t in self.silence.until(pts_ms) {
            self.sink
                .send(flv::TAG_AUDIO, t as u32, &flv::audio_body(&aac::SILENT_FRAME))
                .await?;
        }
        let slices: Vec<&[u8]> = nals
            .iter()
            .copied()
            .filter(|n| !matches!(flv::nal_type(n), 7..=9))
            .collect();
        if !slices.is_empty() {
            self.sink
                .send(
                    flv::TAG_VIDEO,
                    ts,
                    &flv::video_body(enc.keyframe, &flv::to_avcc(&slices)),
                )
                .await?;
        }
        self.sink.flush().await
    }

    /// 送り先が切れた理由 (ファイルは切れない)
    pub async fn closed(&mut self) -> anyhow::Error {
        match &mut self.sink {
            Sink::Rtmp(c) => c.closed().await,
            Sink::File(_) => std::future::pending().await,
        }
    }
}
