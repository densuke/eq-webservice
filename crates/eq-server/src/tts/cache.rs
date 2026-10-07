//! 部品ごとのディスクキャッシュ。仕様は docs/tts.md (S3, S4)。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::anyhow;
use sha2::{Digest, Sha256};

use super::budget::Budget;
use super::google::Synth;
use super::wav;

#[derive(Debug)]
pub enum TtsError {
    Budget,
    Failed(anyhow::Error),
}

pub struct Cache<S> {
    dir: PathBuf,
    voice: String,
    synth: S,
    budget: Budget,
    /// 同じ部品の同時合成を 1 回にまとめる鍵
    inflight: std::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// 異なる部品をまたいだ、合成の同時実行数の上限 (S-04)
    synth_slots: tokio::sync::Semaphore,
}

/// 合成 (Google への呼び出し) の同時実行数。EEW の数秒を守るため少なすぎず、予算の予約が効く範囲に留める
pub const SYNTH_CONCURRENCY: usize = 4;

impl<S: Synth> Cache<S> {
    /// 設定の既定の声
    pub fn default_voice(&self) -> &str {
        &self.voice
    }

    pub fn new(dir: PathBuf, voice: String, synth: S, budget: Budget) -> Self {
        Cache {
            dir,
            voice,
            synth,
            budget,
            inflight: Default::default(),
            synth_slots: tokio::sync::Semaphore::new(SYNTH_CONCURRENCY),
        }
    }

    fn key(&self, text: &str, voice: &str) -> String {
        let d = Sha256::digest(format!("{voice}\n{text}"));
        d.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// キャッシュにあって読めれば返す。壊れていれば None (作り直す)。
    async fn read_cached(path: &std::path::Path) -> Option<Vec<i16>> {
        let bytes = tokio::fs::read(path).await.ok()?;
        wav::parse(&bytes).ok()
    }

    pub async fn segment(&self, text: &str, voice: Option<&str>) -> Result<Vec<i16>, TtsError> {
        let voice = voice.unwrap_or(&self.voice);
        let key = self.key(text, voice);
        let path = self.dir.join("seg").join(format!("{key}.wav"));
        if let Some(pcm) = Self::read_cached(&path).await {
            return Ok(pcm);
        }

        let lock = self.inflight.lock().unwrap().entry(key.clone()).or_default().clone();
        let result = {
            let _guard = lock.lock().await;
            self.synth_and_store(text, voice, &path).await
        };
        // 使い終わった鍵は捨てる (待っている人は自分の Arc を持っている)
        self.inflight.lock().unwrap().remove(&key);
        result
    }

    async fn synth_and_store(&self, text: &str, voice: &str, path: &std::path::Path) -> Result<Vec<i16>, TtsError> {
        // 合成の同時実行数の枠。待っている間に別の人が作り終えているかもしれないので、取ってから確かめる
        let _slot = self
            .synth_slots
            .acquire()
            .await
            .map_err(|e| TtsError::Failed(e.into()))?;
        if let Some(pcm) = Self::read_cached(path).await {
            return Ok(pcm);
        }
        // 使用済み + 予約中の文字数で確かめて予約する。失敗・時間切れ・キャンセルでは Drop で返る。
        // 呼び出し側の時間切れでは合成を止めない (announce) ので、続く合成は予約も持ち続ける
        let chars = text.chars().count();
        let reservation = self
            .budget
            .reserve(chars, SystemTime::now())
            .map_err(|_| TtsError::Budget)?;
        let pcm = self.synth.synth(text, voice).await.map_err(TtsError::Failed)?;
        if let Err(e) = reservation.commit(SystemTime::now()) {
            tracing::warn!("TTS の使用量を記録できない: {e:#}");
        }
        if let Err(e) = write_atomic(path, &wav::encode(&pcm)).await {
            tracing::warn!("TTS のキャッシュを書けない: {e:#}");
        }
        Ok(pcm)
    }

    pub async fn announce(
        self: &Arc<Self>,
        segs: &[String],
        per_segment_timeout: Duration,
    ) -> Result<Vec<u8>, TtsError> {
        let mut parts = Vec::new();
        let mut all_budget = true;
        for seg in segs {
            let this = self.clone();
            let text = seg.clone();
            // 時間切れでも abort しない。裏で合成を終えてキャッシュに残す
            let task = tokio::spawn(async move { this.segment(&text, None).await });
            match tokio::time::timeout(per_segment_timeout, task).await {
                Ok(Ok(Ok(pcm))) => parts.push(pcm),
                Ok(Ok(Err(TtsError::Budget))) => {}
                _ => all_budget = false,
            }
        }
        if parts.is_empty() {
            return Err(if all_budget && !segs.is_empty() {
                TtsError::Budget
            } else {
                TtsError::Failed(anyhow!("all segments failed"))
            });
        }
        Ok(wav::encode(&wav::join(&parts, 150)))
    }
}

/// announce_cached が組み立てる出力の上限 (部品の数)。
/// 正当な最大の報 (大津波警報で全 66 予報区) は約 70 部品。
pub const MAX_ANNOUNCE_SEGMENTS: usize = 128;
/// 同じく総サンプル数の上限 (300 秒)。n2 の実キャッシュの部品は平均 2.6 秒・最大 5.6 秒で、
/// 全 66 予報区の大津波警報は見出し・無音込みで 200 秒前後 (部品が 3.5 秒でも 250 秒前後) になる。
/// メモリ: 300 秒は 13.2M サンプル = 26.5MB。出力の WAV (22.05kHz なら半分) を 1 つのバッファに直接書き、
/// 部品の PCM は 1 つ (最大 約 5.6 秒 = 0.5MB) ずつ読んで捨てるので、組み立て中も送信中も 1 本あたり最大 約 27MB (22.05kHz で約 13MB)。
/// 同時 2 本で 約 54MB で、平常 約 37MB と合わせても eq-server の MemoryMax (256MB) に十分収まる (以前は 1 本 約 53MB)。
pub const MAX_ANNOUNCE_SAMPLES: usize = 300 * wav::RATE as usize;
/// 部品の間の無音 (ms)
const GAP_MS: u32 = 150;

/// 組み立てる出力が上限を超える。
#[derive(Debug, PartialEq, Eq)]
pub struct TooLong;

impl<S: Synth> Cache<S> {
    /// キャッシュ済みの部品だけで組み立てた WAV を返す (half なら 22.05kHz)。合成も予算も使わない。無い部品は飛ばし、1 つも無ければ Ok(None)。
    /// 部品の数と総サンプル数が上限を超えるときは、PCM を読み込む前 (ファイルの大きさだけ見て) に Err。
    /// 出力の Vec に部品を 1 つずつ直接書く。PCM を連結した大きなバッファは作らない。
    pub async fn announce_wav(&self, segs: &[String], half: bool) -> Result<Option<Vec<u8>>, TooLong> {
        if segs.len() > MAX_ANNOUNCE_SEGMENTS {
            return Err(TooLong);
        }
        let mut found = Vec::new();
        let mut samples = 0usize;
        for seg in segs {
            let path = self.dir.join("seg").join(format!("{}.wav", self.key(seg, &self.voice)));
            // 自分で書いた WAV は 44 バイトのヘッダ + 2 バイト/サンプル
            if let Ok(meta) = tokio::fs::metadata(&path).await {
                samples += (meta.len() as usize).saturating_sub(44) / 2;
                found.push(path);
            }
        }
        let gap = wav::RATE as usize * GAP_MS as usize / 1000;
        let total = samples + found.len().saturating_sub(1) * gap;
        if total > MAX_ANNOUNCE_SAMPLES {
            return Err(TooLong);
        }
        let mut out = wav::Sink::new(total, if half { wav::RATE / 2 } else { wav::RATE });
        let mut any = false;
        for path in &found {
            if let Some(pcm) = Self::read_cached(path).await {
                if any {
                    out.silence(gap);
                }
                out.extend(&pcm);
                any = true;
            }
        }
        Ok(any.then(|| out.finish()))
    }
}

/// 一時ファイルに書いてから rename する (途中の壊れたファイルを残さない)。
async fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    tokio::fs::write(&tmp, bytes).await?;
    tokio::fs::rename(&tmp, path).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tts::wav;
    use std::collections::{HashMap, HashSet};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Instant, SystemTime};

    const DEFAULT_VOICE: &str = "ja-JP-Neural2-B";

    /// 呼び出し回数を数える偽の合成器。
    #[derive(Clone, Default)]
    struct Fake {
        calls: Arc<AtomicUsize>,
        fail: Arc<HashSet<String>>,
        sleep: Arc<HashMap<String, Duration>>,
        /// 同時に合成中の数と、その最大
        running: Arc<AtomicUsize>,
        max_running: Arc<AtomicUsize>,
    }

    impl Fake {
        fn failing(texts: &[&str]) -> Fake {
            Fake {
                fail: Arc::new(texts.iter().map(|s| s.to_string()).collect()),
                ..Default::default()
            }
        }
        fn sleeping(text: &str, d: Duration) -> Fake {
            Fake::sleeping_all(&[text], d)
        }
        fn sleeping_all(texts: &[&str], d: Duration) -> Fake {
            Fake {
                sleep: Arc::new(texts.iter().map(|t| (t.to_string(), d)).collect()),
                ..Default::default()
            }
        }
        fn count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl Synth for Fake {
        async fn synth(&self, text: &str, _voice: &str) -> anyhow::Result<Vec<i16>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_running.fetch_max(now, Ordering::SeqCst);
            if let Some(d) = self.sleep.get(text) {
                tokio::time::sleep(*d).await;
            }
            self.running.fetch_sub(1, Ordering::SeqCst);
            if self.fail.contains(text) {
                anyhow::bail!("合成失敗 (テスト)");
            }
            Ok(vec![text.chars().count() as i16; 10])
        }
    }

    fn make(dir: &std::path::Path, limit: usize, fake: Fake) -> Arc<Cache<Fake>> {
        let budget = Budget::load(dir.join("usage.json"), limit);
        Arc::new(Cache::new(dir.to_path_buf(), DEFAULT_VOICE.to_string(), fake, budget))
    }

    fn wav_count(dir: &std::path::Path) -> usize {
        match std::fs::read_dir(dir.join("seg")) {
            Ok(rd) => rd
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|x| x == "wav"))
                .count(),
            Err(_) => 0,
        }
    }

    #[tokio::test]
    async fn 同じ部品は2回目から合成しない() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = Fake::default();
        let cache = make(tmp.path(), 1000, fake.clone());
        let a = cache.segment("あいう", None).await.unwrap();
        let b = cache.segment("あいう", None).await.unwrap();
        assert_eq!(fake.count(), 1);
        assert_eq!(a, b);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn 同時の同じ依頼は合成1回で済む() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = Fake::sleeping("あいう", Duration::from_millis(200));
        let cache = make(tmp.path(), 1000, fake.clone());
        let (c1, c2) = (cache.clone(), cache.clone());
        let (a, b) = tokio::join!(
            tokio::spawn(async move { c1.segment("あいう", None).await }),
            tokio::spawn(async move { c2.segment("あいう", None).await }),
        );
        assert_eq!(a.unwrap().unwrap(), b.unwrap().unwrap());
        assert_eq!(fake.count(), 1);
    }

    #[tokio::test]
    async fn 予算超過は合成せずファイルも作らない() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = Fake::default();
        let cache = make(tmp.path(), 2, fake.clone());
        let r = cache.segment("あいう", None).await;
        assert!(matches!(r, Err(TtsError::Budget)));
        assert_eq!(fake.count(), 0);
        assert_eq!(wav_count(tmp.path()), 0);
    }

    #[tokio::test]
    async fn キャッシュに当たった分は予算を使わない() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = make(tmp.path(), 1000, Fake::default());
        let used = || Budget::load(tmp.path().join("usage.json"), 1000).used(SystemTime::now());
        cache.segment("あいう", None).await.unwrap();
        assert_eq!(used(), 3);
        cache.segment("あいう", None).await.unwrap();
        assert_eq!(used(), 3);
    }

    #[tokio::test]
    async fn 声の指定がなければ既定の声と同じファイル() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = Fake::default();
        let cache = make(tmp.path(), 1000, fake.clone());
        cache.segment("あいう", None).await.unwrap();
        cache.segment("あいう", Some(DEFAULT_VOICE)).await.unwrap();
        assert_eq!(fake.count(), 1);
        assert_eq!(wav_count(tmp.path()), 1);
        cache.segment("あいう", Some("ja-JP-Neural2-C")).await.unwrap();
        assert_eq!(fake.count(), 2);
        assert_eq!(wav_count(tmp.path()), 2);
    }

    #[tokio::test]
    async fn announceは失敗した部品を飛ばして結合する() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = make(tmp.path(), 1000, Fake::failing(&["bb"]));
        let segs = vec!["a".to_string(), "bb".to_string(), "ccc".to_string()];
        let bytes = cache.announce(&segs, Duration::from_secs(1)).await.unwrap();
        // 部品2つ + 間 150ms (44100 * 0.15 = 6615 サンプル) が 1 つ
        assert_eq!(wav::parse(&bytes).unwrap().len(), 10 + 10 + 6615);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn 時間切れの部品は飛ばし裏で合成して保存する() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = Fake::sleeping("遅い", Duration::from_secs(1));
        let cache = make(tmp.path(), 1000, fake.clone());
        let segs = vec!["a".to_string(), "遅い".to_string()];
        let t = Instant::now();
        let bytes = cache.announce(&segs, Duration::from_millis(100)).await.unwrap();
        assert!(t.elapsed() < Duration::from_millis(500));
        assert_eq!(wav::parse(&bytes).unwrap().len(), 10);

        tokio::time::sleep(Duration::from_millis(1500)).await;
        let before = fake.count();
        cache.segment("遅い", None).await.unwrap();
        assert_eq!(fake.count(), before);
    }

    #[tokio::test]
    async fn 全部品が失敗したらエラー() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = make(tmp.path(), 1000, Fake::failing(&["a", "b"]));
        let segs = vec!["a".to_string(), "b".to_string()];
        let r = cache.announce(&segs, Duration::from_secs(1)).await;
        assert!(matches!(r, Err(TtsError::Failed(_))));
    }

    #[tokio::test]
    async fn announce_cached_returns_none_when_nothing_cached() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = Fake::default();
        let cache = make(tmp.path(), 1000, fake.clone());
        let segs = vec!["A".to_string(), "B".to_string()];
        assert_eq!(cache.announce_wav(&segs, false).await, Ok(None));
        assert_eq!(fake.count(), 0);
    }

    #[tokio::test]
    async fn announce_cached_uses_only_cached_segments_without_synth() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = Fake::default();
        let cache = make(tmp.path(), 1000, fake.clone());
        cache.segment("A", None).await.unwrap();
        let before = fake.count();
        let segs = vec!["A".to_string(), "B".to_string()];
        let pcm = wav::parse(&cache.announce_wav(&segs, false).await.unwrap().unwrap()).unwrap();
        // A だけ (間は入らない)
        assert_eq!(pcm.len(), 10);
        assert_eq!(fake.count(), before);
    }

    #[tokio::test]
    async fn announce_cached_rejects_too_many_segments_without_reading() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = make(tmp.path(), 1000, Fake::default());
        let segs = vec!["A".to_string(); MAX_ANNOUNCE_SEGMENTS + 1];
        assert_eq!(cache.announce_wav(&segs, false).await, Err(TooLong));
    }

    #[tokio::test]
    async fn announce_wav_is_byte_identical_to_joining_the_pcm_then_encoding() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = make(tmp.path(), 1000, Fake::default());
        // 長さ 3・8・5 (奇数をまぜて、半分にするときの組が部品の境目でずれる)
        let texts = ["abc", "defghijk", "lmnop"];
        let mut parts = Vec::new();
        for t in texts {
            parts.push(cache.segment(t, None).await.unwrap());
        }
        let segs: Vec<String> = texts.iter().map(|t| t.to_string()).collect();
        let all = wav::join(&parts, GAP_MS);
        assert_eq!(
            cache.announce_wav(&segs, false).await.unwrap().unwrap(),
            wav::encode(&all)
        );
        assert_eq!(
            cache.announce_wav(&segs, true).await.unwrap().unwrap(),
            wav::encode_half(&all)
        );
    }

    // ---- S-04: 予算の予約と、合成の同時実行数 ----

    #[tokio::test]
    async fn 上限3文字に異なる3文字を同時に2本頼むと1本だけ通る() {
        // 監査 S-04 の再現。以前は両方通って 6 文字使えていた
        let tmp = tempfile::tempdir().unwrap();
        let fake = Fake::sleeping_all(&["あいう", "えおか"], Duration::from_millis(100));
        let cache = make(tmp.path(), 3, fake.clone());
        let (a, b) = tokio::join!(cache.segment("あいう", None), cache.segment("えおか", None));
        assert_eq!([a.is_ok(), b.is_ok()].iter().filter(|x| **x).count(), 1);
        assert!(matches!(a.as_ref().err().or(b.as_ref().err()), Some(TtsError::Budget)));
        assert_eq!(fake.count(), 1);
        let used = Budget::load(tmp.path().join("usage.json"), 3).used(SystemTime::now());
        assert_eq!(used, 3);
    }

    #[tokio::test]
    async fn 合成に失敗した予約は返る() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = make(tmp.path(), 3, Fake::failing(&["あいう"]));
        assert!(matches!(cache.segment("あいう", None).await, Err(TtsError::Failed(_))));
        cache.segment("えおか", None).await.unwrap();
    }

    #[tokio::test]
    async fn キャンセルされた合成の予約は返る() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = make(tmp.path(), 3, Fake::sleeping("あいう", Duration::from_secs(5)));
        let r = tokio::time::timeout(Duration::from_millis(50), cache.segment("あいう", None)).await;
        assert!(r.is_err());
        cache.segment("えおか", None).await.unwrap();
    }

    #[tokio::test]
    async fn 時間切れの後も続く合成は予約を持ち続け終われば返す() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = Fake {
            sleep: Arc::new(HashMap::from([("あいう".to_string(), Duration::from_millis(300))])),
            fail: Arc::new(HashSet::from(["あいう".to_string()])),
            ..Default::default()
        };
        let cache = make(tmp.path(), 3, fake);
        let r = cache.announce(&["あいう".to_string()], Duration::from_millis(50)).await;
        assert!(matches!(r, Err(TtsError::Failed(_))));
        // 裏の合成がまだ動いている間は枠が空かない
        assert!(matches!(cache.segment("えおか", None).await, Err(TtsError::Budget)));
        tokio::time::sleep(Duration::from_millis(500)).await;
        // 失敗で終わったので返っている
        cache.segment("えおか", None).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn 異なる部品をまたいだ合成の同時実行数に上限がある() {
        let tmp = tempfile::tempdir().unwrap();
        let texts: Vec<String> = (0..12).map(|i| format!("部品{i}")).collect();
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let fake = Fake::sleeping_all(&refs, Duration::from_millis(50));
        let cache = make(tmp.path(), 10_000, fake.clone());
        let tasks: Vec<_> = texts
            .into_iter()
            .map(|t| {
                let c = cache.clone();
                tokio::spawn(async move { c.segment(&t, None).await })
            })
            .collect();
        for t in tasks {
            t.await.unwrap().unwrap();
        }
        assert_eq!(fake.count(), 12);
        let max = fake.max_running.load(Ordering::SeqCst);
        assert!((2..=SYNTH_CONCURRENCY).contains(&max), "max_running={max}");
    }
}
