//! 配信の音を eq-server の中で作る (mixer)。ページ (?broadcast=1&audio=mixer) は音を鳴らさず、window.eqBroadcast(JSON) で
//! 「BGM を流す・止める」「警戒音」を知らせてくる (chrome.rs が受ける)。
//! ここでは BGM (Icecast の MP3 を戻したもの)・警戒音 (合成)・読み上げの声 (WAV を取ってきたもの。鳴る間は BGM を絞る) を足し、20ms ごとに実時間の速さで PCM
//! (s16le・44.1kHz・ステレオ) を名前付きパイプへ出す。ffmpeg はそれを音の入力として読む。

mod bgm;
mod synth;

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use tokio::net::unix::pipe;
use tokio::sync::mpsc;

use bgm::{BgmStream, PcmSource};
pub use synth::AlertLevel;

/// 音の形式: 44.1kHz・ステレオ・i16 (L R の交互)
pub const RATE: u32 = 44_100;
/// 1 回に出す長さ (20ms)
const TICK: Duration = Duration::from_millis(20);
const TICK_FRAMES: usize = (RATE as usize) / 50;
/// BGM を流し始めるとき、音を上げていく長さ (フレーム数 = 1 秒)
const FADE_IN_FRAMES: usize = RATE as usize;
/// 音量の知らせが無いときの BGM の音量 (画面の既定と同じ 40%)
const DEFAULT_VOLUME: f32 = 0.4;
/// 同時に鳴らせる警戒音の数 (ページが暴走しても音を積み上げない)
const MAX_ALERTS: usize = 16;
/// 声が鳴っている間の BGM の倍率
const DUCK: f32 = 0.3;
/// 声の待ち行列の長さ (あふれたら古いものを捨てる)
const MAX_VOICES: usize = 4;

/// ページからの知らせ (docs/broadcast-v2.md の W1)
#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Notice {
    Bgm {
        play: bool,
        volume: Option<f32>,
    },
    Alert {
        level: AlertLevel,
    },
    /// 読み上げの音声 (WAV の URL)
    Voice {
        url: String,
    },
}

struct Bgm {
    source: Box<dyn PcmSource>,
    volume: f32,
    /// 流し始めてからのフレーム数 (立ち上がりの計算に使う)
    played: usize,
}

struct Playing {
    pcm: Vec<i16>,
    pos: usize,
}

pub struct AudioMixer {
    open_bgm: Box<dyn Fn() -> Option<Box<dyn PcmSource>> + Send>,
    bgm: Option<Bgm>,
    alerts: Vec<Playing>,
    voices: VecDeque<Playing>,
}

impl AudioMixer {
    /// open_bgm: BGM を流し始めるときに、そのときの放送につなぐ (URL が無ければ None)
    pub fn new(open_bgm: impl Fn() -> Option<Box<dyn PcmSource>> + Send + 'static) -> Self {
        AudioMixer {
            open_bgm: Box::new(open_bgm),
            bgm: None,
            alerts: Vec::new(),
            voices: VecDeque::new(),
        }
    }

    pub fn apply(&mut self, n: Notice) {
        match n {
            Notice::Bgm { play: true, volume } => {
                let volume = volume.unwrap_or(DEFAULT_VOLUME).clamp(0.0, 1.0);
                match &mut self.bgm {
                    Some(b) => b.volume = volume,
                    None => {
                        self.bgm = (self.open_bgm)().map(|source| Bgm {
                            source,
                            volume,
                            played: 0,
                        })
                    }
                }
            }
            // 止めるときはすぐ (受信もやめる)
            Notice::Bgm { play: false, .. } => self.bgm = None,
            Notice::Alert { level } if self.alerts.len() < MAX_ALERTS => {
                self.alerts.push(Playing {
                    pcm: synth::alert(level),
                    pos: 0,
                });
            }
            Notice::Alert { .. } => {}
            // 声は取ってくるのが非同期なので run() が受けて push_voice する
            Notice::Voice { .. } => {}
        }
    }

    /// 警戒音 1 回の長さ (フレーム数)。再現動画が、声を警戒音のあとに置くのに使う
    pub fn alert_frames(level: AlertLevel) -> usize {
        synth::alert(level).len() / 2
    }

    /// 声 (モノ) を待ち行列に積む。ステレオ (L=R) に広げる。待ちは 4 本まで (あふれたら古いものを捨てる)
    pub fn push_voice(&mut self, mono: Vec<i16>) {
        let pcm = mono.into_iter().flat_map(|s| [s, s]).collect();
        self.voices.push_back(Playing { pcm, pos: 0 });
        if self.voices.len() > MAX_VOICES {
            self.voices.pop_front();
        }
    }

    /// frames フレーム分 (L R の交互) を混ぜて出す。足し算が i16 を超えたら飽和させる
    pub fn render(&mut self, frames: usize) -> Vec<i16> {
        let mut sum = vec![0i32; frames * 2];
        // 声: 先頭の 1 本だけ鳴らし、終わったら同じ chunk の続きから次を鳴らす
        let mut has_voice = vec![false; frames];
        let mut f = 0;
        while f < frames {
            let Some(v) = self.voices.front_mut() else {
                break;
            };
            let n = ((v.pcm.len() - v.pos) / 2).min(frames - f);
            for (s, x) in sum[f * 2..(f + n) * 2].iter_mut().zip(&v.pcm[v.pos..v.pos + n * 2]) {
                *s += *x as i32;
            }
            has_voice[f..f + n].fill(true);
            v.pos += n * 2;
            f += n;
            if v.pos >= v.pcm.len() {
                self.voices.pop_front();
            }
        }
        if let Some(b) = &mut self.bgm {
            let mut buf = vec![0i16; frames * 2];
            b.source.pull(&mut buf); // 足りない分は無音のまま
            for (i, s) in buf.iter().enumerate() {
                let ramp = ((b.played + i / 2) as f32 / FADE_IN_FRAMES as f32).min(1.0);
                let duck = if has_voice[i / 2] { DUCK } else { 1.0 };
                sum[i] += (*s as f32 * b.volume * ramp * duck) as i32;
            }
            b.played += frames;
        }
        for a in &mut self.alerts {
            let n = (a.pcm.len() - a.pos).min(sum.len());
            for (s, x) in sum.iter_mut().zip(&a.pcm[a.pos..a.pos + n]) {
                *s += *x as i32;
            }
            a.pos += n;
        }
        self.alerts.retain(|a| a.pos < a.pcm.len());
        sum.into_iter()
            .map(|s| s.clamp(i16::MIN as i32, i16::MAX as i32) as i16)
            .collect()
    }
}

/// 動かしている mixer。捨てると止まる (BGM の受信もやめる)
pub struct Running(tokio::task::JoinHandle<()>);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub fn spawn(notices: mpsc::UnboundedReceiver<String>, fifo: PathBuf, bgm_url: String) -> Running {
    Running(tokio::spawn(run(notices, fifo, bgm_url)))
}

/// fifo の書き込み側を、ffmpeg が読み始めるまで待って開く。
/// 普通の open は読み手が来るまで止まったまま取り消せず、ffmpeg が立ち上がらなかったときに終了まで巻き込むので、
/// 読み手がいなければ失敗する開き方 (ENXIO) をやり直す
async fn open_fifo(path: &Path) -> std::io::Result<pipe::Sender> {
    const ENXIO: i32 = 6; // macOS・Linux とも 6
    loop {
        match pipe::OpenOptions::new().open_sender(path) {
            Err(e) if e.raw_os_error() == Some(ENXIO) => tokio::time::sleep(Duration::from_millis(20)).await,
            other => return other,
        }
    }
}

fn is_not_found(e: &anyhow::Error) -> bool {
    e.downcast_ref::<reqwest::Error>().and_then(|e| e.status()) == Some(reqwest::StatusCode::NOT_FOUND)
}

/// 声の WAV を取ってきて、モノの PCM にして送る。失敗したら警告だけ出して鳴らさない
async fn fetch_voice(client: reqwest::Client, url: String, tx: mpsc::UnboundedSender<Vec<i16>>) {
    let res = match crate::net::body(client.get(&url)).await {
        Ok(bytes) => crate::tts::wav::parse(&bytes),
        Err(e) => Err(e),
    };
    match res {
        Ok(pcm) => {
            let _ = tx.send(pcm);
        }
        // 404 は、その報に読み上げが無い (サーバの tts が無効など)。異常ではないので静かに諦める
        Err(e) if is_not_found(&e) => tracing::debug!("broadcast: no voice for this report: {url}"),
        Err(e) => tracing::warn!("broadcast: voice fetch failed ({e}): {url}"),
    }
}

/// 知らせ (JSON の文字列) を受けて混ぜ、実時間の速さで PCM を fifo に書き続ける。ffmpeg が読み始めるまでは待つ。
/// 書けなくなった (ffmpeg が止まった) ら終わる
async fn run(mut notices: mpsc::UnboundedReceiver<String>, fifo: PathBuf, bgm_url: String) {
    let open = move || (!bgm_url.is_empty()).then(|| Box::new(BgmStream::start(bgm_url.clone())) as Box<dyn PcmSource>);
    let mut mixer = AudioMixer::new(open);
    let http = crate::net::client(Duration::from_secs(10));
    let (voice_tx, mut voice_rx) = mpsc::unbounded_channel::<Vec<i16>>();
    let Ok(mut out) = open_fifo(&fifo).await else {
        return;
    };
    let mut tick = tokio::time::interval(TICK);
    // 遅れたら、あとで詰めて出す (音を抜かない。平均が実時間になる)
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);
    loop {
        tokio::select! {
            _ = tick.tick() => {
                let bytes: Vec<u8> = mixer.render(TICK_FRAMES).into_iter().flat_map(i16::to_le_bytes).collect();
                if out.write_all(&bytes).await.is_err() {
                    return;
                }
            }
            Some(pcm) = voice_rx.recv() => mixer.push_voice(pcm),
            Some(json) = notices.recv() => match serde_json::from_str::<Notice>(&json) {
                Ok(Notice::Voice { url }) => match &http {
                    Ok(c) => {
                        tokio::spawn(fetch_voice(c.clone(), url, voice_tx.clone()));
                    }
                    Err(e) => tracing::warn!("broadcast: voice client unavailable ({e}): {url}"),
                },
                Ok(n) => mixer.apply(n),
                Err(e) => {
                    let head: String = json.chars().take(100).collect();
                    tracing::warn!("broadcast: unknown notice from the page ({e}): {head}");
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 決まった値を出し続ける音 (BGM の代わり)
    struct Constant(i16);
    impl PcmSource for Constant {
        fn pull(&mut self, out: &mut [i16]) -> usize {
            out.fill(self.0);
            out.len()
        }
    }

    fn mixer_with_bgm(level: i16) -> AudioMixer {
        AudioMixer::new(move || Some(Box::new(Constant(level))))
    }

    fn play(volume: f32) -> Notice {
        Notice::Bgm {
            play: true,
            volume: Some(volume),
        }
    }

    /// 1 秒ぶん流して、そのあとの 1 フレームを返す (立ち上がりが終わった状態)
    fn settled(m: &mut AudioMixer) -> Vec<i16> {
        m.render(RATE as usize);
        m.render(1)
    }

    #[test]
    fn silence_without_anything_playing() {
        let mut m = mixer_with_bgm(1000);
        assert_eq!(m.render(100), vec![0; 200]);
    }

    #[test]
    fn bgm_is_scaled_by_the_volume() {
        let mut m = mixer_with_bgm(10_000);
        m.apply(play(0.4));
        assert_eq!(settled(&mut m), [4000, 4000]);
        m.apply(play(0.5)); // 流している間の音量の知らせは、そのまま効く
        assert_eq!(m.render(1), [5000, 5000]);
    }

    #[test]
    fn bgm_rises_over_one_second() {
        let mut m = mixer_with_bgm(10_000);
        m.apply(play(1.0));
        let first = m.render(RATE as usize);
        let at = |sec: f32| first[(sec * RATE as f32) as usize * 2] as f32;
        assert_eq!(first[0], 0);
        assert!((at(0.25) - 2500.0).abs() < 5.0, "{}", at(0.25));
        assert!((at(0.5) - 5000.0).abs() < 5.0, "{}", at(0.5));
        assert!((at(0.99) - 9900.0).abs() < 20.0, "{}", at(0.99));
        assert_eq!(m.render(1), [10_000, 10_000]);
    }

    #[test]
    fn stopping_is_immediate_and_starting_again_rises_again() {
        let mut m = mixer_with_bgm(10_000);
        m.apply(play(1.0));
        settled(&mut m);
        m.apply(Notice::Bgm {
            play: false,
            volume: None,
        });
        assert_eq!(m.render(10), vec![0; 20]);
        m.apply(play(1.0));
        assert_eq!(m.render(1), [0, 0]);
    }

    #[test]
    fn alerts_add_to_the_bgm_and_saturate_instead_of_wrapping() {
        let mut m = mixer_with_bgm(30_000);
        m.apply(play(1.0));
        settled(&mut m);
        let mut alone = mixer_with_bgm(0);
        for mixer in [&mut m, &mut alone] {
            for _ in 0..4 {
                mixer.apply(Notice::Alert { level: AlertLevel::Low });
            }
        }
        let n = RATE as usize / 2;
        let (with_bgm, only_alert) = (m.render(n), alone.render(n));
        assert!(
            only_alert.iter().any(|&x| (x as i32 + 30_000) > i16::MAX as i32),
            "the test must overflow"
        );
        for (a, b) in with_bgm.iter().zip(&only_alert) {
            let want = (30_000 + *b as i32).clamp(i16::MIN as i32, i16::MAX as i32);
            assert_eq!(*a as i32, want);
        }
    }

    #[test]
    fn an_alert_plays_once_and_then_ends() {
        let mut m = mixer_with_bgm(0);
        m.apply(Notice::Alert { level: AlertLevel::Pip });
        let out = m.render(RATE as usize / 4); // pip は 0.12 秒
        assert!(out.iter().any(|&x| x != 0));
        assert_eq!(&out[out.len() - 200..], &[0; 200]);
        assert!(m.alerts.is_empty());
    }

    #[test]
    fn a_short_bgm_source_is_padded_with_silence() {
        struct Short;
        impl PcmSource for Short {
            fn pull(&mut self, out: &mut [i16]) -> usize {
                out[..4].fill(1000);
                4
            }
        }
        let mut m = AudioMixer::new(|| Some(Box::new(Short)));
        m.apply(play(1.0));
        m.render(RATE as usize);
        let out = m.render(4);
        assert_eq!(&out[..4], &[1000; 4]);
        assert_eq!(&out[4..], &[0; 4]);
    }

    #[test]
    fn notices_from_the_page_are_parsed() {
        let p = |s: &str| serde_json::from_str::<Notice>(s);
        assert_eq!(p(r#"{"type":"bgm","play":true,"volume":0.4}"#).unwrap(), play(0.4));
        assert_eq!(
            p(r#"{"type":"bgm","play":false}"#).unwrap(),
            Notice::Bgm {
                play: false,
                volume: None
            }
        );
        assert_eq!(
            p(r#"{"type":"alert","level":"strong"}"#).unwrap(),
            Notice::Alert {
                level: AlertLevel::Strong
            }
        );
        assert!(p(r#"{"type":"alert","level":"loud"}"#).is_err());
        assert!(p(r#"{"type":"reboot"}"#).is_err());
    }

    #[tokio::test]
    async fn the_fifo_opens_when_a_reader_appears_and_can_be_given_up_before() {
        use tokio::io::AsyncReadExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pcm");
        assert!(std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success());
        // 読み手がいない間は待つだけで、取り消せる (終了を巻き込まない)
        let waiting = tokio::time::timeout(Duration::from_millis(100), open_fifo(&path)).await;
        assert!(waiting.is_err());
        // 読み手が来たら開けて、書いたものが届く
        let mut reader = pipe::OpenOptions::new().open_receiver(&path).unwrap();
        let mut writer = open_fifo(&path).await.unwrap();
        writer.write_all(&[1, 2, 3]).await.unwrap();
        let mut got = [0u8; 3];
        reader.read_exact(&mut got).await.unwrap();
        assert_eq!(got, [1, 2, 3]);
    }

    #[test]
    fn no_bgm_url_means_no_bgm() {
        let mut m = AudioMixer::new(|| None);
        m.apply(play(1.0));
        assert!(m.bgm.is_none());
    }

    #[tokio::test]
    async fn a_404_is_told_apart_from_other_failures() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for (status, not_found) in [("404 Not Found", true), ("503 Service Unavailable", false)] {
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/", l.local_addr().unwrap());
            tokio::spawn(async move {
                let (mut c, _) = l.accept().await.unwrap();
                let _ = c.read(&mut [0u8; 1024]).await;
                let _ = c
                    .write_all(
                        format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes(),
                    )
                    .await;
            });
            let e = crate::net::body(reqwest::Client::new().get(&url)).await.unwrap_err();
            assert_eq!(is_not_found(&e), not_found, "{status}");
        }
    }

    #[test]
    fn a_voice_notice_is_parsed() {
        let n = serde_json::from_str::<Notice>(r#"{"type":"voice","url":"http://x/a.wav"}"#).unwrap();
        assert_eq!(
            n,
            Notice::Voice {
                url: "http://x/a.wav".into()
            }
        );
    }

    /// L と R を取り出して、フレームごとの値にする (L=R であることも確かめる)
    fn mono_of(out: &[i16]) -> Vec<i16> {
        out.chunks(2)
            .map(|f| {
                assert_eq!(f[0], f[1], "L と R は同じ");
                f[0]
            })
            .collect()
    }

    #[test]
    fn voices_play_one_after_another_never_overlapping() {
        let mut m = AudioMixer::new(|| None);
        m.push_voice(vec![1000; 30]);
        m.push_voice(vec![2000; 20]);
        let got = mono_of(&m.render(70));
        assert_eq!(&got[..30], &[1000; 30]);
        assert_eq!(&got[30..50], &[2000; 20]);
        assert_eq!(&got[50..], &[0; 20]);
    }

    #[test]
    fn bgm_is_ducked_only_while_a_voice_plays() {
        let mut m = mixer_with_bgm(10_000);
        m.apply(play(1.0));
        settled(&mut m);
        m.push_voice(vec![0; 50]); // 無音の声でも「鳴っている」扱い
        let got = mono_of(&m.render(60));
        for (i, v) in got.iter().enumerate() {
            let want = if i < 50 { 3000 } else { 10_000 };
            assert!((*v as i32 - want).abs() <= 2, "frame {i}: {v}");
        }
    }

    #[test]
    fn at_most_four_voices_wait_and_the_oldest_is_dropped() {
        let mut m = AudioMixer::new(|| None);
        for v in 1..=5i16 {
            m.push_voice(vec![v; 10]);
        }
        let got = mono_of(&m.render(60));
        let want: Vec<i16> = (2..=5i16).flat_map(|v| [v; 10]).chain([0; 20]).collect();
        assert_eq!(got, want);
    }
}
