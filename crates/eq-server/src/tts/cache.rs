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
}

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
        // 待っている間に別の人が作り終えているかもしれない
        if let Some(pcm) = Self::read_cached(path).await {
            return Ok(pcm);
        }
        let chars = text.chars().count();
        self.budget
            .check(chars, SystemTime::now())
            .map_err(|_| TtsError::Budget)?;
        let pcm = self.synth.synth(text, voice).await.map_err(TtsError::Failed)?;
        if let Err(e) = self.budget.record(chars, SystemTime::now()) {
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
/// 同じく総サンプル数の上限 (180 秒)。全 66 予報区の大津波警報は、1 部品 約 1.5〜2 秒でも約 2 分に収まる。
/// 出力は 16 MB 弱 (PCM と WAV で約 2 倍、ブラウザ向けの 22.05kHz 化で更に増える)。
/// 同時 3 本でも eq-server の MemoryMax (256MB) に収まる大きさ。
pub const MAX_ANNOUNCE_SAMPLES: usize = 180 * wav::RATE as usize;
/// 部品の間の無音 (ms)
const GAP_MS: u32 = 150;

/// 組み立てる出力が上限を超える。
#[derive(Debug, PartialEq, Eq)]
pub struct TooLong;

impl<S: Synth> Cache<S> {
    /// キャッシュ済みの部品だけで組み立てる。合成も予算も使わない。無い部品は飛ばし、1 つも無ければ Ok(None)。
    /// 部品の数と総サンプル数が上限を超えるときは、PCM を読み込む前 (ファイルの大きさだけ見て) に Err。
    pub async fn announce_cached(&self, segs: &[String]) -> Result<Option<Vec<u8>>, TooLong> {
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
        let gaps = found.len().saturating_sub(1) * (wav::RATE as usize * GAP_MS as usize / 1000);
        if samples + gaps > MAX_ANNOUNCE_SAMPLES {
            return Err(TooLong);
        }
        let mut parts = Vec::new();
        for path in &found {
            if let Some(pcm) = Self::read_cached(path).await {
                parts.push(pcm);
            }
        }
        if parts.is_empty() {
            return Ok(None);
        }
        Ok(Some(wav::encode(&wav::join(&parts, GAP_MS))))
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
    }

    impl Fake {
        fn failing(texts: &[&str]) -> Fake {
            Fake {
                fail: Arc::new(texts.iter().map(|s| s.to_string()).collect()),
                ..Default::default()
            }
        }
        fn sleeping(text: &str, d: Duration) -> Fake {
            Fake {
                sleep: Arc::new(HashMap::from([(text.to_string(), d)])),
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
            if let Some(d) = self.sleep.get(text) {
                tokio::time::sleep(*d).await;
            }
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
        assert_eq!(cache.announce_cached(&segs).await, Ok(None));
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
        let bytes = cache.announce_cached(&segs).await.unwrap().unwrap();
        // A だけ (間は入らない)
        assert_eq!(wav::parse(&bytes).unwrap().len(), 10);
        assert_eq!(fake.count(), before);
    }

    #[tokio::test]
    async fn announce_cached_rejects_too_many_segments_without_reading() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = make(tmp.path(), 1000, Fake::default());
        let segs = vec!["A".to_string(); MAX_ANNOUNCE_SEGMENTS + 1];
        assert_eq!(cache.announce_cached(&segs).await, Err(TooLong));
    }
}
