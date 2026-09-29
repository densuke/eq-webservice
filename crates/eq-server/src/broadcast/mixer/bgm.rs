//! BGM の受信と戻し。Icecast の MP3 (44.1kHz・ステレオで用意してある) を受けながら PCM (44.1kHz・ステレオ・i16) に戻す。
//! 途切れたら (切れた・止まった・壊れた) 5 秒後につなぎ直す。BgmStream を捨てると受信もやめる。
//! symphonia は同期の読み出しなので、HTTP は tokio で読み、戻しは別のスレッドで行う (途中に上限付きの通り道を置く)。

use std::io::Read;
use std::time::Duration;

use anyhow::Context;
use symphonia::core::audio::sample::Sample;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSourceStream, ReadOnlySource};
use symphonia::core::meta::MetadataOptions;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::RATE;

/// 途切れてからつなぎ直すまで
const RETRY_AFTER: Duration = Duration::from_secs(5);
/// 受信が止まったとみなす無音の長さ
const STALL: Duration = Duration::from_secs(15);
/// 戻した音をためておく数 (1 つは約 26ms。約 1.7 秒。あふれる前に受信が待たされる)
const QUEUE: usize = 64;

/// 音を一定の速さで引き取る側 (mixer)。足りなければ 0 (無音) のまま
pub trait PcmSource: Send {
    /// out に詰められるだけ詰めて、詰めたサンプル数 (L R の交互) を返す。待たない
    fn pull(&mut self, out: &mut [i16]) -> usize;
}

pub struct BgmStream {
    rx: mpsc::Receiver<Vec<i16>>,
    rest: Vec<i16>,
    pos: usize,
    task: JoinHandle<()>,
}

impl BgmStream {
    /// つなぎ始める (その時点の放送から)。つながるまでの間は pull が 0 を返す
    pub fn start(url: String) -> BgmStream {
        let (tx, rx) = mpsc::channel(QUEUE);
        let task = tokio::spawn(async move {
            while !tx.is_closed() {
                match receive(&url, &tx).await {
                    Ok(()) => tracing::warn!("bgm: stream ended"),
                    Err(e) => tracing::warn!("bgm: {e:#}"),
                }
                tokio::time::sleep(RETRY_AFTER).await;
            }
        });
        BgmStream {
            rx,
            rest: Vec::new(),
            pos: 0,
            task,
        }
    }
}

impl PcmSource for BgmStream {
    fn pull(&mut self, out: &mut [i16]) -> usize {
        let mut n = 0;
        while n < out.len() {
            if self.pos == self.rest.len() {
                match self.rx.try_recv() {
                    Ok(next) => (self.rest, self.pos) = (next, 0),
                    Err(_) => break,
                }
            }
            let k = (out.len() - n).min(self.rest.len() - self.pos);
            out[n..n + k].copy_from_slice(&self.rest[self.pos..self.pos + k]);
            self.pos += k;
            n += k;
        }
        n
    }
}

impl Drop for BgmStream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// 1 回つないで、切れるまで受けて戻す
async fn receive(url: &str, pcm: &mpsc::Sender<Vec<i16>>) -> anyhow::Result<()> {
    // 全体の時間は区切らない (ずっと流れ続ける)。止まったかは 1 回の待ちで見る
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .user_agent(concat!("eq-webservice/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let mut res = client.get(url).send().await?.error_for_status()?;
    let (bytes_tx, bytes_rx) = mpsc::channel::<Vec<u8>>(8);
    let pcm = pcm.clone();
    let decoder = tokio::task::spawn_blocking(move || {
        decode(ChannelReader::new(bytes_rx), |chunk| pcm.blocking_send(chunk).is_ok())
    });
    loop {
        let chunk = tokio::time::timeout(STALL, res.chunk())
            .await
            .context("no data from the stream")??;
        let Some(chunk) = chunk else { break };
        // 戻す側が終わっていれば (壊れた・受け取る側が無くなった) 受信もやめる
        if bytes_tx.send(chunk.to_vec()).await.is_err() {
            break;
        }
    }
    drop(bytes_tx);
    decoder.await.context("decoder thread")?
}

/// 受け取ったバイト列を Read として読ませる
struct ChannelReader {
    rx: mpsc::Receiver<Vec<u8>>,
    rest: Vec<u8>,
    pos: usize,
}

impl ChannelReader {
    fn new(rx: mpsc::Receiver<Vec<u8>>) -> Self {
        ChannelReader {
            rx,
            rest: Vec::new(),
            pos: 0,
        }
    }
}

impl Read for ChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        while self.pos == self.rest.len() {
            match self.rx.blocking_recv() {
                Some(next) => (self.rest, self.pos) = (next, 0),
                None => return Ok(0),
            }
        }
        let k = buf.len().min(self.rest.len() - self.pos);
        buf[..k].copy_from_slice(&self.rest[self.pos..self.pos + k]);
        self.pos += k;
        Ok(k)
    }
}

/// MP3 を最後まで (または on_chunk が false を返すまで) 戻す。on_chunk には 44.1kHz・ステレオ・i16 の音が順に渡る
fn decode(src: impl Read + Send + Sync + 'static, mut on_chunk: impl FnMut(Vec<i16>) -> bool) -> anyhow::Result<()> {
    let mss = MediaSourceStream::new(Box::new(ReadOnlySource::new(src)), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .context("not an MP3 stream")?;
    let track = format.default_track(TrackType::Audio).context("no audio track")?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .context("no audio parameters")?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .context("creating the MP3 decoder")?;
    let track_id = track.id;
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => return Ok(()),
            // 送り元が切れた (途中で終わった) のは正常な終わりとして扱い、つなぎ直しに任せる
            Err(Error::IoError(_)) => return Ok(()),
            Err(e) => return Err(e).context("reading the stream"),
        };
        if packet.track_id != track_id {
            continue;
        }
        let buf = match decoder.decode(&packet) {
            Ok(b) => b,
            // 壊れたフレームは飛ばす
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(e).context("decoding"),
        };
        let spec = buf.spec();
        anyhow::ensure!(spec.rate() == RATE, "BGM is {}Hz, expected {RATE}Hz", spec.rate());
        let channels = spec.channels().count();
        let mut samples = vec![i16::MID; buf.samples_interleaved()];
        buf.copy_to_slice_interleaved(&mut samples);
        if !on_chunk(to_stereo(&samples, channels)) {
            return Ok(());
        }
    }
}

/// n チャンネルの交互のサンプルを、ステレオの交互にする (1 なら両方に、3 以上は先頭の 2 つ)
fn to_stereo(samples: &[i16], channels: usize) -> Vec<i16> {
    match channels {
        2 => samples.to_vec(),
        1 => samples.iter().flat_map(|&x| [x, x]).collect(),
        n => samples.chunks(n).flat_map(|f| [f[0], f[1]]).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ffmpeg で作った 440Hz の正弦波 (44.1kHz・ステレオ・2 秒)
    const SINE: &[u8] = include_bytes!("testdata/sine.mp3");

    fn decode_all(src: &'static [u8]) -> Vec<i16> {
        let mut all = Vec::new();
        decode(src, |c| {
            all.extend(c);
            true
        })
        .unwrap();
        all
    }

    #[test]
    fn a_two_second_mp3_becomes_two_seconds_of_stereo() {
        let pcm = decode_all(SINE);
        let frames = pcm.len() / 2;
        // MP3 は 1152 フレーム単位で、エンコーダの先頭に少し余白が付く
        let (want, slack) = (2 * RATE as usize, 3 * 1152);
        assert!((want..=want + slack).contains(&frames), "{frames} frames");
        assert_eq!(pcm.len() % 2, 0);
    }

    #[test]
    fn the_decoded_sound_is_the_sine_wave() {
        let pcm = decode_all(SINE);
        let mid = &pcm[RATE as usize..RATE as usize + 2000]; // 1 秒あたりの 1000 フレーム
        let peak = mid.iter().map(|&x| (x as i32).abs()).max().unwrap();
        assert!(peak > 1500 && peak < 6000, "peak {peak}");
        // 440Hz の正弦波なら、1000 フレーム (約 23ms) の間に 10 回ほど (符号が 20 回ほど) 変わる
        let left: Vec<i16> = mid.iter().step_by(2).copied().collect();
        let crossings = left.windows(2).filter(|w| (w[0] < 0) != (w[1] < 0)).count();
        assert!((17..=23).contains(&crossings), "{crossings} crossings");
    }

    #[test]
    fn decoding_stops_when_the_receiver_is_gone() {
        let mut calls = 0;
        decode(SINE, |_| {
            calls += 1;
            false
        })
        .unwrap();
        assert_eq!(calls, 1);
    }

    #[test]
    fn a_stream_cut_in_the_middle_ends_without_error() {
        let all = decode_all(&SINE[..SINE.len() / 2]);
        assert!(all.len() / 2 > RATE as usize / 2);
        assert!(all.len() / 2 < 2 * RATE as usize);
    }

    #[test]
    fn something_that_is_not_mp3_is_an_error() {
        assert!(decode(&b"<html>not found</html>"[..], |_| true).is_err());
    }

    #[test]
    fn channel_counts_become_stereo() {
        assert_eq!(to_stereo(&[1, 2, 3, 4], 2), [1, 2, 3, 4]);
        assert_eq!(to_stereo(&[1, 2], 1), [1, 1, 2, 2]);
        assert_eq!(to_stereo(&[1, 2, 9, 3, 4, 9], 3), [1, 2, 3, 4]);
    }

    #[tokio::test]
    async fn pull_takes_what_is_there_and_never_waits() {
        let (tx, rx) = mpsc::channel(4);
        let mut s = BgmStream {
            rx,
            rest: Vec::new(),
            pos: 0,
            task: tokio::spawn(async {}),
        };
        let mut out = [0i16; 6];
        assert_eq!(s.pull(&mut out), 0);
        tx.send(vec![1, 2, 3, 4]).await.unwrap();
        tx.send(vec![5, 6, 7, 8]).await.unwrap();
        assert_eq!(s.pull(&mut out), 6);
        assert_eq!(out, [1, 2, 3, 4, 5, 6]);
        assert_eq!(s.pull(&mut out), 2);
        assert_eq!(&out[..2], [7, 8]);
    }
}
