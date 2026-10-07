//! 起動時の事前合成。docs/tts.md S1 を参照

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::cache::{Cache, TtsError};
use super::google::Synth;
use crate::quake::Event;

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
        )
        .chain(super::phrase::userquake_pref_segments());
    let mut seen = HashSet::new();
    all.filter(|s| seen.insert(s.clone())).collect()
}

/// 記録の全イベントを順に読み上げたときに現れる部品 (重複なし、初出順)。docs/tts.md S10。
pub fn record_segments(events: &[Event]) -> Vec<String> {
    let all = events
        .iter()
        .flat_map(|ev| super::phrase::announce_segments(ev, &super::priors::priors_of(ev, events)));
    dedupe(all)
}

/// 初出順を保って重複を除く。
fn dedupe(it: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    it.into_iter().filter(|s| seen.insert(s.clone())).collect()
}

/// JSON Lines の Event を読む (旧ファイル + 日付ファイルの全部。archive_store)。壊れた行・空行は飛ばし、ファイルがなければ空。
pub fn load_jsonl(path: &Path) -> Vec<Event> {
    let files = crate::archive_store::files_in_range(path, 0, u64::MAX).unwrap_or_default();
    let mut broken = 0;
    let events = files
        .iter()
        .filter_map(|f| {
            std::fs::read_to_string(f)
                .map_err(|e| tracing::warn!("tts prewarm: {} を読めない: {e}", f.display()))
                .ok()
        })
        .flat_map(|text| {
            text.lines()
                .filter(|l| !l.trim().is_empty())
                .filter_map(|l| serde_json::from_str::<Event>(l).map_err(|_| broken += 1).ok())
                .collect::<Vec<_>>()
        })
        .collect();
    if broken > 0 {
        tracing::warn!("tts prewarm: {} の壊れた行を {broken} 件飛ばした", path.display());
    }
    events
}

#[derive(serde::Deserialize)]
struct Scenario {
    events: Vec<Event>,
}

/// デモのディレクトリから、シナリオごと (index.json 以外の *.json、名前順) の Event 列を読む。
pub fn load_demo_dir(dir: &Path) -> Vec<Vec<Event>> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<_> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json") && p.file_name().is_some_and(|n| n != "index.json"))
        .collect();
    files.sort();
    files
        .iter()
        .filter_map(|p| {
            let sc = std::fs::read_to_string(p)
                .map_err(|e| e.to_string())
                .and_then(|t| serde_json::from_str::<Scenario>(&t).map_err(|e| e.to_string()));
            match sc {
                Ok(sc) => Some(sc.events),
                Err(e) => {
                    tracing::warn!("tts prewarm: デモ {} を読めない: {e}", p.display());
                    None
                }
            }
        })
        .collect()
}

/// 固定の部品 + 記録 + デモ の合成対象一覧 (重複なし、初出順)。ファイルを読むので blocking。
fn build_list(archive: Option<&Path>, demo_dir: Option<&Path>) -> Vec<String> {
    let fixed = prewarm_segments();
    let arch = archive.map(|p| record_segments(&load_jsonl(p))).unwrap_or_default();
    let demo: Vec<String> = demo_dir
        .map(|d| load_demo_dir(d).iter().flat_map(|sc| record_segments(sc)).collect())
        .unwrap_or_default();
    let counts = (fixed.len(), arch.len(), demo.len());
    let all = dedupe(fixed.into_iter().chain(arch).chain(demo));
    tracing::info!(
        "tts prewarm list: fixed={} archive={} demo={} total={}",
        counts.0,
        counts.1,
        counts.2,
        all.len()
    );
    all
}

/// 起動時に事前合成をバックグラウンドで始める。
pub fn spawn_prewarm<S: Synth>(cache: Arc<Cache<S>>, archive: Option<PathBuf>, demo_dir: Option<PathBuf>) {
    tokio::spawn(async move {
        let segs = tokio::task::spawn_blocking(move || build_list(archive.as_deref(), demo_dir.as_deref()))
            .await
            .unwrap_or_default();
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
    fn contains_userquake_phrases() {
        let v = prewarm_segments();
        for s in [
            "揺れを感じたという報告が集まっています。",
            "茨城県で、",
            "茨城県、",
            "千葉県などで、",
        ] {
            assert!(v.iter().any(|x| x == s), "missing: {s}");
        }
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

    fn ev_json(id: &str) -> String {
        // 実データの先頭イベントを id だけ差し替えて使う
        let v: serde_json::Value = serde_json::from_str(NOTO).unwrap();
        let mut e = v["events"][0].clone();
        e["id"] = serde_json::Value::String(id.into());
        e.to_string()
    }

    const NOTO: &str = include_str!("../../../../web/public/demo/noto2024.json");

    fn noto_events() -> Vec<Event> {
        let v: serde_json::Value = serde_json::from_str(NOTO).unwrap();
        serde_json::from_value(v["events"].clone()).unwrap()
    }

    #[test]
    fn load_jsonl_skips_broken_and_blank_lines() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("e.jsonl");
        let body = format!("{}\nnot json\n\n{}\n", ev_json("a"), ev_json("b"));
        std::fs::write(&p, body).unwrap();
        assert_eq!(load_jsonl(&p).len(), 2);
    }

    #[test]
    fn load_jsonl_reads_the_legacy_file_and_the_daily_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("e.jsonl"), format!("{}\n", ev_json("a"))).unwrap();
        std::fs::write(dir.path().join("e-2026-10-07.jsonl"), format!("{}\n", ev_json("b"))).unwrap();
        assert_eq!(load_jsonl(&dir.path().join("e.jsonl")).len(), 2);
    }

    #[test]
    fn load_jsonl_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_jsonl(&dir.path().join("none.jsonl")).is_empty());
    }

    #[test]
    fn load_demo_dir_keeps_scenarios_separate() {
        let dir = tempfile::tempdir().unwrap();
        let w = |name: &str, evs: &[&str]| {
            let body = format!("{{\"events\":[{}]}}", evs.join(","));
            std::fs::write(dir.path().join(name), body).unwrap();
        };
        let (a1, a2, b1) = (ev_json("a1"), ev_json("a2"), ev_json("b1"));
        w("a.json", &[&a1, &a2]);
        w("b.json", &[&b1]);
        w("index.json", &[&a1]);
        std::fs::write(dir.path().join("broken.json"), "{oops").unwrap();
        let groups = load_demo_dir(dir.path());
        let lens: Vec<usize> = groups.iter().map(Vec::len).collect();
        assert_eq!(lens, vec![2, 1]);
    }

    #[test]
    fn load_demo_dir_missing_dir_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_demo_dir(&dir.path().join("none")).is_empty());
    }

    #[test]
    fn record_segments_of_noto_has_expected_parts_without_duplicates() {
        let v = record_segments(&noto_events());
        for s in [
            "続報。",
            "石川県で震度7を観測しました。",
            "大津波警報を発表しました。",
            "石川県能登。",
            "震源は石川県能登地方。",
        ] {
            assert!(v.iter().any(|x| x == s), "missing: {s}");
        }
        let set: std::collections::HashSet<_> = v.iter().collect();
        assert_eq!(set.len(), v.len());
        let total: usize = v.iter().map(|s| s.chars().count()).sum();
        println!("noto record segments: {} / chars {total}", v.len());
    }

    #[test]
    fn demo_record_extra_segments_stay_within_cost_guard() {
        let base: std::collections::HashSet<String> = prewarm_segments().into_iter().collect();
        let groups = load_demo_dir(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/public/demo")));
        assert!(!groups.is_empty(), "demo dir not found");
        let mut extra = std::collections::BTreeSet::new();
        for g in &groups {
            extra.extend(record_segments(g).into_iter().filter(|s| !base.contains(s)));
        }
        let total: usize = extra.iter().map(|s| s.chars().count()).sum();
        println!("extra segments ({} / chars {total}):", extra.len());
        for s in &extra {
            println!("  {s}");
        }
        assert!(total < 5_000, "extra chars {total}");
    }
}
