//! 起動時の事前合成。docs/tts.md S1 を参照

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::cache::{Cache, TtsError};
use super::google::Synth;

/// 事前合成する部品の一覧 (重複なし、初出順)。
pub fn prewarm_segments() -> Vec<String> {
    let lines = |t: &'static str| t.lines().map(str::trim).filter(|l| !l.is_empty());
    let all = super::phrase::fixed_segments()
        .into_iter()
        .chain(lines(include_str!("epicenters.txt")).map(|n| format!("震源は{n}。")))
        .chain(lines(include_str!("tsunami_areas.txt")).map(|a| format!("{a}。")))
        .chain(
            crate::quake::area::PREFS
                .iter()
                .map(|p| format!("{p}などで揺れを観測しました。")),
        );
    let mut seen = HashSet::new();
    all.filter(|s| seen.insert(s.clone())).collect()
}

/// 起動時に事前合成をバックグラウンドで始める。
pub fn spawn_prewarm<S: Synth>(cache: Arc<Cache<S>>) {
    tokio::spawn(async move {
        let segs = prewarm_segments();
        let total = segs.len();
        for (i, s) in segs.iter().enumerate() {
            let t = Instant::now();
            match cache.segment(s, None).await {
                Ok(_) => {}
                Err(TtsError::Budget) => {
                    tracing::warn!("tts prewarm: 文字数予算が尽きたため中断 {i}/{total}");
                    return;
                }
                Err(e) => tracing::warn!("tts prewarm: 合成失敗 {s}: {e:?}"),
            }
            // キャッシュヒットは即時なので、5ms 超かかった (=合成した) ときだけ間隔を空ける
            if t.elapsed() > Duration::from_millis(5) {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            let done = i + 1;
            if done % 50 == 0 {
                tracing::info!("tts prewarm {done}/{total}");
            }
        }
        tracing::info!("tts prewarm {total}/{total}");
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tts::phrase::fixed_segments;

    const SUFFIX: &str = "などで揺れを観測しました。";

    #[test]
    fn no_duplicates() {
        let v = prewarm_segments();
        let set: std::collections::HashSet<_> = v.iter().collect();
        assert_eq!(set.len(), v.len());
    }

    #[test]
    fn contains_expected_segments() {
        let v = prewarm_segments();
        for s in [
            "震源は能登半島沖。",
            "震源は石川県能登地方。",
            "伊勢・三河湾。",
            "石川県などで揺れを観測しました。",
            "北海道などで揺れを観測しました。",
        ] {
            assert!(v.iter().any(|x| x == s), "missing: {s}");
        }
        for f in fixed_segments() {
            assert!(v.contains(&f), "missing fixed: {f}");
        }
    }

    #[test]
    fn exactly_47_prefecture_segments() {
        let n = prewarm_segments().iter().filter(|s| s.ends_with(SUFFIX)).count();
        assert_eq!(n, 47);
    }

    #[test]
    fn no_blank_entries() {
        for s in prewarm_segments() {
            assert_ne!(s, "震源は。");
            assert_ne!(s, "。");
        }
    }

    #[test]
    fn total_chars_within_budget() {
        let total: usize = prewarm_segments().iter().map(|s| s.chars().count()).sum();
        println!("prewarm total chars: {total}");
        assert!(total < 20_000, "total {total}");
    }
}
