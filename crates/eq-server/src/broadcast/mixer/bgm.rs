//! BGM の受信と戻し。Icecast の MP3 (44.1kHz・ステレオで用意してある) を受けながら PCM (44.1kHz・ステレオ・i16) に戻す。
//! 途切れたら (切れた・止まった・壊れた) 5 秒後につなぎ直す。BgmStream を捨てると受信もやめる。
//! symphonia は同期の読み出しなので、HTTP は tokio で読み、戻しは別のスレッドで行う (途中に上限付きの通り道を置く)。

use std::io::Read;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

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
/// 受け取りの状況を報告する間隔
const REPORT_EVERY: Duration = Duration::from_secs(60);
/// 戻した音をためておく数 (1 つは約 26ms。約 1.7 秒。あふれる前に受信が待たされる)
const QUEUE: usize = 64;

/// 音を一定の速さで引き取る側 (mixer)。足りなければ 0 (無音) のまま
pub trait PcmSource: Send {
    /// out に詰められるだけ詰めて、詰めたサンプル数 (L R の交互) を返す。待たない
    fn pull(&mut self, out: &mut [i16]) -> usize;
}

/// 受け取りの足りなさを数える (調整はせず、見えるようにするだけ)。純粋なので単体でテストできる
#[derive(Debug, Default, PartialEq)]
struct Stats {
    /// 要求より少なかった pull の回数
    underruns: u64,
    /// 足りなかったサンプル数の合計
    short_samples: u64,
}

impl Stats {
    fn record(&mut self, wanted: usize, got: usize) {
        if got < wanted {
            self.underruns += 1;
            self.short_samples += (wanted - got) as u64;
        }
    }

    /// 1 行の報告。buffered_samples は L R の交互のサンプル数
    fn line(&self, buffered_samples: usize, reconnects: u64) -> String {
        let secs = buffered_samples as f64 / (RATE as f64 * 2.0);
        format!(
            "bgm: underruns={} short_samples={} buffered={secs:.2}s reconnects={reconnects}",
            self.underruns, self.short_samples
        )
    }
}

/// 受け取る側 (tokio のタスク) と使う側 (pull) で共有する数
#[derive(Default)]
struct Shared {
    /// 通り道にたまっているサンプル数
    queued: AtomicUsize,
    /// つなぎ直した回数
    reconnects: AtomicU64,
}

pub struct BgmStream {
    rx: mpsc::Receiver<Vec<i16>>,
    rest: Vec<i16>,
    pos: usize,
    task: JoinHandle<()>,
    shared: Arc<Shared>,
    stats: Stats,
    last_report: Instant,
}

impl BgmStream {
    /// つなぎ始める (その時点の放送から)。つながるまでの間は pull が 0 を返す
    pub fn start(url: String) -> BgmStream {
        let (tx, rx) = mpsc::channel(QUEUE);
        let shared = Arc::new(Shared::default());
        let task_shared = shared.clone();
        let task = tokio::spawn(async move {
            while !tx.is_closed() {
                match receive(&url, &tx, task_shared.clone()).await {
                    Ok(()) => tracing::warn!("bgm: stream ended"),
                    Err(e) => tracing::warn!("bgm: {e:#}"),
                }
                task_shared.reconnects.fetch_add(1, Ordering::Relaxed);
                tokio::time::sleep(RETRY_AFTER).await;
            }
        });
        BgmStream::with(rx, task, shared)
    }

    fn with(rx: mpsc::Receiver<Vec<i16>>, task: JoinHandle<()>, shared: Arc<Shared>) -> BgmStream {
        BgmStream {
            rx,
            rest: Vec::new(),
            pos: 0,
            task,
            shared,
            stats: Stats::default(),
            last_report: Instant::now(),
        }
    }

    fn buffered(&self) -> usize {
        self.shared.queued.load(Ordering::Relaxed) + (self.rest.len() - self.pos)
    }
}

impl PcmSource for BgmStream {
    fn pull(&mut self, out: &mut [i16]) -> usize {
        let mut n = 0;
        while n < out.len() {
            if self.pos == self.rest.len() {
                match self.rx.try_recv() {
                    Ok(next) => {
                        self.shared.queued.fetch_sub(next.len(), Ordering::Relaxed);
                        (self.rest, self.pos) = (next, 0);
                    }
                    Err(_) => break,
                }
            }
            let k = (out.len() - n).min(self.rest.len() - self.pos);
            out[n..n + k].copy_from_slice(&self.rest[self.pos..self.pos + k]);
            self.pos += k;
            n += k;
        }
        self.stats.record(out.len(), n);
        // 流している間 (pull が呼ばれている間) だけ、1 分ごとに報告する
        if self.last_report.elapsed() >= REPORT_EVERY {
            self.last_report = Instant::now();
            let reconnects = self.shared.reconnects.load(Ordering::Relaxed);
            tracing::info!("{}", self.stats.line(self.buffered(), reconnects));
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
async fn receive(url: &str, pcm: &mpsc::Sender<Vec<i16>>, shared: Arc<Shared>) -> anyhow::Result<()> {
    // 全体の時間は区切らない (ずっと流れ続ける)。止まったかは 1 回の待ちで見る
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .user_agent(concat!("eq-webservice/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let mut res = client.get(url).send().await?.error_for_status()?;
    let (bytes_tx, bytes_rx) = mpsc::channel::<Vec<u8>>(8);
    let pcm = pcm.clone();
    let decoder = tokio::task::spawn_blocking(move || {
        decode(ChannelReader::new(bytes_rx), |chunk| {
            let n = chunk.len();
            shared.queued.fetch_add(n, Ordering::Relaxed);
            let sent = pcm.blocking_send(chunk).is_ok();
            if !sent {
                shared.queued.fetch_sub(n, Ordering::Relaxed);
            }
            sent
        })
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
        let mut s = BgmStream::with(rx, tokio::spawn(async {}), Arc::default());
        let mut out = [0i16; 6];
        assert_eq!(s.pull(&mut out), 0);
        tx.send(vec![1, 2, 3, 4]).await.unwrap();
        tx.send(vec![5, 6, 7, 8]).await.unwrap();
        assert_eq!(s.pull(&mut out), 6);
        assert_eq!(out, [1, 2, 3, 4, 5, 6]);
        assert_eq!(s.pull(&mut out), 2);
        assert_eq!(&out[..2], [7, 8]);
    }

    #[test]
    fn short_pulls_are_counted_and_reported_in_one_line() {
        let mut st = Stats::default();
        st.record(10, 10);
        st.record(10, 4);
        st.record(10, 0);
        assert_eq!(
            st,
            Stats {
                underruns: 2,
                short_samples: 16
            }
        );
        // 1.5 秒ぶん (44100 * 2 * 1.5 サンプル) ためている
        assert_eq!(
            st.line(132_300, 3),
            "bgm: underruns=2 short_samples=16 buffered=1.50s reconnects=3"
        );
    }

    #[tokio::test]
    async fn buffered_counts_the_queue_and_the_unused_rest() {
        let (tx, rx) = mpsc::channel(4);
        let shared = Arc::new(Shared::default());
        let mut s = BgmStream::with(rx, tokio::spawn(async {}), shared.clone());
        shared.queued.fetch_add(8, Ordering::Relaxed);
        tx.send(vec![0; 4]).await.unwrap();
        tx.send(vec![0; 4]).await.unwrap();
        assert_eq!(s.buffered(), 8);
        s.pull(&mut [0i16; 2]); // 1 つ目 (4) を受けて 2 使った
        assert_eq!(s.buffered(), 6);
    }
}
