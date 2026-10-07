//! 再現動画の読み上げ (docs/replay-video.md 7 章)。
//! サーバの `POST /api/tts/announce` (キャッシュ済みの部品だけで組み立てる。Google は呼ばない) から声をもらい、
//! 警戒音のあとに置く。ブラウザの履歴の再生 (web/src/voice.ts の announceBody) と同じ本文を送る。

use std::time::Duration;

use anyhow::Context;

use super::plan::Frame;
use super::sound::alert_level;
use crate::broadcast::mixer::AudioMixer;
use crate::broadcast::native::model::should_read_without_alert;
use crate::quake::{Event, EventBody};
use crate::tts::{priors::priors_of, wav};

/// 続報に添える先行報の上限 (サーバの MAX_PRIORS と同じ)
const MAX_PRIORS: usize = 300;
/// 要求と要求の間の休み (eq.fuga.jp は小さな VM)
pub const PAUSE: Duration = Duration::from_millis(300);
/// 動画の 1 回の再生ごとの番号 (デモの `#demoN`。動画は 1 本ずつなので 1 で足りる)
const RUN: u32 = 1;

/// 読む報 1 つ。at_ms は動画の中で声を始める時刻
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    pub index: usize,
    pub at_ms: u64,
}

/// 取れた声 (44.1kHz モノ)
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    pub at_ms: u64,
    pub pcm: Vec<i16>,
}

/// サーバから声を取る口 (テストでは偽物にする)。Ok(None) は「読むものがない」(404)
pub trait Announce {
    async fn announce(&self, body: String) -> anyhow::Result<Option<Vec<i16>>>;
}

/// 読む報とその時刻。警戒音を鳴らす報と、鳴らさなくても読む津波予報 (配信・ブラウザと同じ規則)。
/// 声は、報が届いたコマの、警戒音の長さのあとに始める
pub fn schedule(events: &[Event], frames: &[Frame], fps: u32) -> Vec<Slot> {
    (0..events.len())
        .filter_map(|i| {
            let e = &events[i];
            let chime_ms = match alert_level(&events[..=i], e.received_at_ms) {
                Some(level) => AudioMixer::alert_frames(level) as u64 * 1000 / u64::from(crate::broadcast::mixer::RATE),
                None if should_read_without_alert(&events[..i], e) => 0,
                None => return None,
            };
            let frame = frames.iter().position(|f| f.applied > i)?;
            Some(Slot {
                index: i,
                at_ms: frame as u64 * 1000 / u64::from(fps) + chime_ms,
            })
        })
        .collect()
}

/// デモの報にする (source を demo、id を `…#demoN`。緊急地震速報は event_id も)
fn demo(e: &Event) -> Event {
    let mut e = e.clone();
    e.id = format!("{}#demo{RUN}", e.id);
    e.source = "demo".into();
    if let EventBody::Eew(x) = &mut e.body {
        x.event_id = format!("{}#demo{RUN}", x.event_id);
    }
    e
}

/// 要求の本文。同じまとまりの前の報 (最大 300) を届いた順に添える
pub fn body(events: &[Event], index: usize) -> anyhow::Result<String> {
    let priors: Vec<Event> = priors_of(&events[index], &events[..=index])
        .into_iter()
        .map(demo)
        .collect();
    let priors = &priors[priors.len().saturating_sub(MAX_PRIORS)..];
    Ok(serde_json::to_string(&serde_json::json!({
        "event": demo(&events[index]),
        "priors": priors,
    }))?)
}

/// 読む報の声を 1 本ずつ取る。404 はその報だけ無音。通信の失敗は警告を 1 回出して、そこまでに取れた分で打ち切る
pub async fn collect(api: &impl Announce, events: &[Event], slots: &[Slot], pause: Duration) -> Vec<Clip> {
    let mut clips = Vec::new();
    for (n, s) in slots.iter().enumerate() {
        if n > 0 {
            tokio::time::sleep(pause).await;
        }
        let reply = match body(events, s.index) {
            Ok(b) => api.announce(b).await,
            Err(e) => Err(e),
        };
        match reply {
            Ok(Some(pcm)) => clips.push(Clip { at_ms: s.at_ms, pcm }),
            Ok(None) => {}
            Err(e) => {
                tracing::warn!("replay-video: 読み上げを取れません。声なしで続けます: {e:#}");
                break;
            }
        }
    }
    clips
}

/// 時刻が来た声を mixer の待ち行列に積む (待ちは 4 本まで。あふれたら古いものを捨てるのは mixer)
pub fn push_due<'a>(
    mixer: &mut AudioMixer,
    clips: &mut std::iter::Peekable<impl Iterator<Item = &'a Clip>>,
    at_ms: u64,
) {
    while let Some(c) = clips.next_if(|c| c.at_ms <= at_ms) {
        mixer.push_voice(c.pcm.clone());
    }
}

/// 本物のサーバ
pub struct Http {
    client: reqwest::Client,
    url: String,
    /// 503 (同時実行の枠が満杯) のあとの待ち
    retry_wait: Duration,
    /// 429 (IP ごとの頻度制限。固定窓は 1 分) のあとの待ち。3 回やり直せば窓を過ぎる長さ
    rate_wait: Duration,
}

/// 503 のやり直しの回数
const RETRIES: usize = 3;

/// 応答の扱い
#[derive(Debug, PartialEq, Eq)]
enum Reply {
    /// 声が届いた
    Voice,
    /// その報だけ無音 (読むものがない 404、長すぎる・上限超過の 413/422)
    Skip,
    /// 枠が満杯 (503)。短く待ってやり直す
    Retry,
    /// 頻度制限 (429)。窓が過ぎるまで待ってやり直す。再現動画は 300ms ごとに頼むので、本番に向けるとここに当たる
    RateLimited,
    /// それ以外は失敗
    Fail,
}

fn classify(status: reqwest::StatusCode) -> Reply {
    match status.as_u16() {
        200..=299 => Reply::Voice,
        404 | 413 | 422 => Reply::Skip,
        503 => Reply::Retry,
        429 => Reply::RateLimited,
        _ => Reply::Fail,
    }
}

impl Http {
    pub fn new(base: &str) -> anyhow::Result<Self> {
        Ok(Http {
            client: crate::net::client(Duration::from_secs(30))?,
            url: format!("{}/api/tts/announce", base.trim_end_matches('/')),
            retry_wait: Duration::from_secs(1),
            rate_wait: Duration::from_secs(30),
        })
    }
}

impl Announce for Http {
    async fn announce(&self, body: String) -> anyhow::Result<Option<Vec<i16>>> {
        for attempt in 0..=RETRIES {
            let res = self
                .client
                .post(&self.url)
                .header("content-type", "application/json")
                .body(body.clone())
                .send()
                .await?;
            match classify(res.status()) {
                Reply::Skip => return Ok(None),
                Reply::Retry if attempt < RETRIES => tokio::time::sleep(self.retry_wait).await,
                Reply::RateLimited if attempt < RETRIES => tokio::time::sleep(self.rate_wait).await,
                Reply::Retry | Reply::RateLimited | Reply::Fail => {
                    res.error_for_status()?;
                    anyhow::bail!("想定外の応答");
                }
                Reply::Voice => {
                    // 声は 1 本で数 MB (数十秒〜数分の 44.1kHz モノ)。サーバは自分たちのもの
                    let bytes = res.bytes().await?;
                    return Ok(Some(wav::parse(&bytes).context("声の WAV を読めません")?));
                }
            }
        }
        unreachable!("最後のやり直しは return か Err で終わる")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use super::super::testkit::*;
    use super::*;
    use crate::broadcast::mixer::RATE;
    use crate::quake::{QuakeInfoType, Scale};

    const T: u64 = T0 as u64;

    fn frame(applied: usize) -> Frame {
        Frame {
            now_ms: 0,
            applied,
            fast_forward: false,
            hindsight: None,
        }
    }

    /// 1 コマ目で 0 件、2 コマ目 (fps 5 なら 200ms) で 1 件、…と届く筋書き
    fn frames(n: usize) -> Vec<Frame> {
        (0..=n).map(frame).collect()
    }

    /// 偽のサーバ: 本文を覚え、決まった答えを順に返す
    struct Fake {
        replies: Mutex<VecDeque<anyhow::Result<Option<Vec<i16>>>>>,
        bodies: Mutex<Vec<String>>,
    }

    impl Fake {
        fn new(replies: Vec<anyhow::Result<Option<Vec<i16>>>>) -> Self {
            Fake {
                replies: Mutex::new(replies.into()),
                bodies: Mutex::new(Vec::new()),
            }
        }
    }

    impl Announce for Fake {
        async fn announce(&self, body: String) -> anyhow::Result<Option<Vec<i16>>> {
            self.bodies.lock().unwrap().push(body);
            self.replies.lock().unwrap().pop_front().unwrap_or(Ok(None))
        }
    }

    #[test]
    fn only_reports_that_ring_an_alert_are_read() {
        let ev = [
            eew("E", 1, T, T0, false, Scale::S4, TOKYO),         // 最初の報: 読む
            eew("E", 2, T + 1_000, T0, false, Scale::S4, TOKYO), // 震度が上がらない続報: 読まない
            as_test(eew("F", 1, T + 2_000, T0, false, Scale::S4, TOKYO)), // 訓練: 読まない
            as_cancelled(eew("E", 3, T + 3_000, T0, false, Scale::S4, TOKYO)), // 取り消し: 読まない
            quake(T + 90_000, QuakeInfoType::Destination, T0, Scale::S4, Some(TOKYO)), // 地震情報の最初: 読む
        ];
        let slots = schedule(&ev, &frames(5), 5);
        assert_eq!(slots.iter().map(|s| s.index).collect::<Vec<_>>(), vec![0, 4]);
    }

    fn tsunami(id: &str, issued: &str, recv: u64) -> Event {
        use crate::quake::model::{Tsunami, TsunamiArea, TsunamiGrade};
        Event {
            id: id.into(),
            source: "test".into(),
            received_at_ms: recv,
            body: EventBody::Tsunami(Tsunami {
                cancelled: false,
                issued_at: issued.into(),
                areas: vec![TsunamiArea {
                    name: "宮城県".into(),
                    grade: TsunamiGrade::Watch,
                    immediate: false,
                    first_height: None,
                    max_height: None,
                }],
            }),
        }
    }

    #[test]
    fn a_first_tsunami_forecast_is_read_without_a_chime_but_its_follow_up_is_not() {
        let ev = [
            tsunami("a", "2026-01-01T00:00", T),
            tsunami("b", "2026-01-01T00:10", T + 1_000),
        ];
        let slots = schedule(&ev, &frames(2), 5);
        assert_eq!(slots, vec![Slot { index: 0, at_ms: 200 }]); // 警戒音がないので、届いたコマそのまま
    }

    #[test]
    fn a_voice_starts_after_the_chime_of_the_frame_that_delivers_the_report() {
        let ev = [eew("E", 1, T, T0, false, Scale::S4, TOKYO)];
        // 3 コマ目 (index 2) で届く。fps 5 なら 400ms
        let fs = vec![frame(0), frame(0), frame(1)];
        let slots = schedule(&ev, &fs, 5);
        let chime =
            AudioMixer::alert_frames(crate::broadcast::mixer::AlertLevel::Medium) as u64 * 1000 / u64::from(RATE);
        assert_eq!(
            slots,
            vec![Slot {
                index: 0,
                at_ms: 400 + chime
            }]
        );
    }

    #[test]
    fn a_report_never_delivered_in_the_video_gets_no_slot() {
        let ev = [eew("E", 1, T, T0, false, Scale::S4, TOKYO)];
        assert!(schedule(&ev, &[frame(0), frame(0)], 5).is_empty());
    }

    #[test]
    fn the_body_marks_events_as_demo_and_lists_earlier_reports_of_the_same_quake_in_order() {
        let ev = [
            eew("E", 1, T, T0, false, Scale::S3, TOKYO),
            eew("OTHER", 1, T + 500, T0, false, Scale::S3, OSAKA),
            eew("E", 2, T + 1_000, T0, false, Scale::S4, TOKYO),
        ];
        let v: serde_json::Value = serde_json::from_str(&body(&ev, 2).unwrap()).unwrap();
        assert_eq!(v["event"]["source"], "demo");
        assert_eq!(v["event"]["id"], "E-2#demo1");
        assert_eq!(v["event"]["event_id"], "E#demo1");
        let priors = v["priors"].as_array().unwrap();
        assert_eq!(priors.len(), 1);
        assert_eq!(
            (priors[0]["id"].as_str(), priors[0]["source"].as_str()),
            (Some("E-1#demo1"), Some("demo"))
        );
        assert_eq!(v.as_object().unwrap().len(), 2); // サーバは event と priors 以外を受けない
    }

    #[test]
    fn the_body_keeps_only_the_latest_300_priors() {
        let ev: Vec<Event> = (1..=305)
            .map(|i| eew("E", i, T + u64::from(i), T0, false, Scale::S3, TOKYO))
            .collect();
        let v: serde_json::Value = serde_json::from_str(&body(&ev, 304).unwrap()).unwrap();
        let priors = v["priors"].as_array().unwrap();
        assert_eq!(priors.len(), 300);
        assert_eq!(priors[0]["id"], "E-5#demo1");
        assert_eq!(priors[299]["id"], "E-304#demo1");
    }

    fn slots_of(n: usize) -> Vec<Slot> {
        (0..n)
            .map(|i| Slot {
                index: i,
                at_ms: i as u64 * 1000,
            })
            .collect()
    }

    fn many_events(n: usize) -> Vec<Event> {
        (1..=n as u32)
            .map(|i| eew("E", i, T + u64::from(i), T0, false, Scale::S3, TOKYO))
            .collect()
    }

    #[tokio::test]
    async fn a_404_leaves_only_that_report_silent() {
        let api = Fake::new(vec![Ok(Some(vec![1; 10])), Ok(None), Ok(Some(vec![2; 10]))]);
        let clips = collect(&api, &many_events(3), &slots_of(3), Duration::ZERO).await;
        assert_eq!(clips.len(), 2);
        assert_eq!((clips[0].at_ms, clips[1].at_ms), (0, 2000));
        assert_eq!(api.bodies.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn a_failure_stops_asking_but_keeps_what_was_fetched() {
        let api = Fake::new(vec![
            Ok(Some(vec![1; 10])),
            Err(anyhow::anyhow!("down")),
            Ok(Some(vec![2; 10])),
        ]);
        let clips = collect(&api, &many_events(3), &slots_of(3), Duration::ZERO).await;
        assert_eq!(clips.len(), 1);
        assert_eq!(api.bodies.lock().unwrap().len(), 2); // 3 本目は要求しない
    }

    #[tokio::test]
    async fn requests_are_spaced_out_and_sequential() {
        let api = Fake::new(vec![]);
        let t = std::time::Instant::now();
        collect(&api, &many_events(3), &slots_of(3), Duration::from_millis(40)).await;
        // 1 本目の前には休まない (間の 2 回だけ)
        assert!(t.elapsed() >= Duration::from_millis(80));
        assert!(t.elapsed() < Duration::from_millis(400));
    }

    #[test]
    fn voices_are_queued_one_at_a_time_and_only_when_due() {
        let clips = [
            Clip {
                at_ms: 0,
                pcm: vec![100; RATE as usize],
            }, // 1 秒
            Clip {
                at_ms: 500,
                pcm: vec![200; RATE as usize],
            }, // 1 秒 (前の声が鳴り終わってから)
        ];
        let mut m = AudioMixer::new(|| None);
        let mut it = clips.iter().peekable();
        push_due(&mut m, &mut it, 0);
        assert_eq!(it.len(), 1); // 2 本目はまだ
        push_due(&mut m, &mut it, 500);
        assert_eq!(it.len(), 0);
        let out = m.render(RATE as usize * 2);
        let (first, second) = (out[0], out[RATE as usize * 2 + 2]);
        assert_eq!((first, second), (100, 200)); // 1 秒目は 1 本目、2 秒目は 2 本目 (重ならない)
    }

    #[tokio::test]
    async fn the_http_client_posts_to_the_announce_path_and_reads_wav_or_404() {
        use axum::{http::StatusCode, routing::post, Router};
        let wav_bytes = wav::encode(&[5, 6, 7]);
        let app = Router::new().route(
            "/api/tts/announce",
            post(move |body: String| {
                let w = wav_bytes.clone();
                async move {
                    if body.contains("nothing") {
                        (StatusCode::NOT_FOUND, Vec::new())
                    } else {
                        (StatusCode::OK, w)
                    }
                }
            }),
        );
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        let h = Http::new(&base).unwrap();
        assert_eq!(h.announce("{}".into()).await.unwrap(), Some(vec![5, 6, 7]));
        assert_eq!(h.announce("nothing".into()).await.unwrap(), None);
        // つながらない
        let down = Http::new("http://127.0.0.1:1").unwrap();
        assert!(down.announce("{}".into()).await.is_err());
    }

    #[test]
    fn classify_skips_only_that_report_for_404_413_422_and_retries_503() {
        use reqwest::StatusCode as S;
        assert_eq!(classify(S::OK), Reply::Voice);
        for c in [S::NOT_FOUND, S::PAYLOAD_TOO_LARGE, S::UNPROCESSABLE_ENTITY] {
            assert_eq!(classify(c), Reply::Skip, "{c}");
        }
        assert_eq!(classify(S::SERVICE_UNAVAILABLE), Reply::Retry);
        assert_eq!(classify(S::TOO_MANY_REQUESTS), Reply::RateLimited);
        assert_eq!(classify(S::INTERNAL_SERVER_ERROR), Reply::Fail);
    }

    /// 決まった応答を順に返すだけの HTTP サーバ。最後の応答は繰り返す。(base URL, 受けた回数)
    async fn serve(replies: Vec<(u16, Vec<u8>)>) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let n = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = n.clone();
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let i = count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let (status, body) = replies[i.min(replies.len() - 1)].clone();
                let mut buf = [0u8; 8192];
                let _ = sock.read(&mut buf).await;
                let head = format!(
                    "HTTP/1.1 {status} X\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = sock.write_all(head.as_bytes()).await;
                let _ = sock.write_all(&body).await;
            }
        });
        (base, n)
    }

    fn http_to(base: &str) -> Http {
        Http {
            retry_wait: Duration::from_millis(10),
            rate_wait: Duration::from_millis(10),
            ..Http::new(base).unwrap()
        }
    }

    #[tokio::test]
    async fn http_skips_the_report_on_413_and_422() {
        for status in [413, 422, 404] {
            let (base, n) = serve(vec![(status, vec![])]).await;
            assert_eq!(http_to(&base).announce("{}".into()).await.unwrap(), None, "{status}");
            assert_eq!(n.load(std::sync::atomic::Ordering::SeqCst), 1);
        }
    }

    #[tokio::test]
    async fn http_retries_503_then_gets_the_voice() {
        let (base, n) = serve(vec![(503, vec![]), (503, vec![]), (200, wav::encode(&[1, 2, 3]))]).await;
        let got = http_to(&base).announce("{}".into()).await.unwrap();
        assert_eq!(got, Some(vec![1, 2, 3]));
        assert_eq!(n.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn http_gives_up_after_three_retries_of_503() {
        let (base, n) = serve(vec![(503, vec![])]).await;
        assert!(http_to(&base).announce("{}".into()).await.is_err());
        assert_eq!(n.load(std::sync::atomic::Ordering::SeqCst), 1 + RETRIES);
    }

    #[tokio::test]
    async fn collect_goes_on_to_the_next_voice_after_a_skipped_report() {
        let (base, _) = serve(vec![(422, vec![]), (200, wav::encode(&[7, 7]))]).await;
        let http = http_to(&base);
        let events = many_events(2);
        let slots = vec![Slot { index: 0, at_ms: 100 }, Slot { index: 1, at_ms: 200 }];
        let clips = collect(&http, &events, &slots, Duration::ZERO).await;
        assert_eq!(
            clips,
            vec![Clip {
                at_ms: 200,
                pcm: vec![7, 7]
            }]
        );
    }

    #[tokio::test]
    async fn http_waits_out_429_instead_of_dropping_the_voice() {
        let (base, n) = serve(vec![(429, vec![]), (429, vec![]), (200, wav::encode(&[4, 5]))]).await;
        let got = http_to(&base).announce("{}".into()).await.unwrap();
        assert_eq!(got, Some(vec![4, 5]));
        assert_eq!(n.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn collect_keeps_every_voice_when_the_server_rate_limits_midway() {
        let (base, _) = serve(vec![(200, wav::encode(&[1])), (429, vec![]), (200, wav::encode(&[2]))]).await;
        let events = many_events(2);
        let slots = vec![Slot { index: 0, at_ms: 100 }, Slot { index: 1, at_ms: 200 }];
        let clips = collect(&http_to(&base), &events, &slots, Duration::ZERO).await;
        assert_eq!(clips.len(), 2);
    }
}
