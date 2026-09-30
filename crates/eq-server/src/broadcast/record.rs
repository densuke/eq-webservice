//! 地震の画面になったときの切り出し (`[record]`、docs/quake-archive.md 3 章)。
//! リングバッファそのものは ffmpeg の tee (segment) が作る。ここはそれを読むだけで、
//! 地震の画面に切り替わって `after_min` 分たったら、`before_min` 分前からの分を 1 本にまとめて archive_dir に置く。
//! 失敗はログに出すだけで、配信は止めない。

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::Deserialize;
use tokio::sync::watch;

/// tee の segment_time (秒)。1 つのファイルが、更新時刻の前のこの長さの分を持つとみなす
const SEGMENT_SECS: u64 = 60;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RecordConfig {
    /// tee の segment が書くディレクトリ (00.ts, 01.ts, ...)
    pub ring_dir: String,
    /// 切り出した動画を置くディレクトリ
    pub archive_dir: String,
    /// 地震の画面になる何分前から残すか
    pub before_min: u64,
    /// 地震の画面になって何分後まで残すか
    pub after_min: u64,
    /// 残す本数 (古いものから消す)
    pub keep: usize,
    /// ディスクの空き (MB) がこれ未満なら切り出さない
    pub min_free_mb: u64,
}

impl Default for RecordConfig {
    fn default() -> Self {
        RecordConfig {
            ring_dir: "ring".into(),
            archive_dir: "archive".into(),
            before_min: 5,
            after_min: 10,
            keep: 20,
            min_free_mb: 500,
        }
    }
}

/// 切り出す時間の範囲 (予約)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Window {
    from: SystemTime,
    until: SystemTime,
}

/// 地震の画面になったときの予約。すでに予約があれば、始まりはそのままで終わりだけ延ばす (1 本にまとめる)
fn reserve(cur: Option<Window>, now: SystemTime, cfg: &RecordConfig) -> Window {
    Window {
        from: cur.map_or(now - mins(cfg.before_min), |w| w.from),
        until: now + mins(cfg.after_min),
    }
}

fn mins(m: u64) -> Duration {
    Duration::from_secs(m * 60)
}

/// ring の中で範囲にかかるファイルを、更新時刻の順に返す
/// (更新時刻はそのファイルの最後の書き込みなので、`SEGMENT_SECS` 前から更新時刻までを持つとみなす)
fn select_segments(files: &[(PathBuf, SystemTime)], w: Window) -> Vec<PathBuf> {
    let mut hit: Vec<_> = files
        .iter()
        .filter(|(p, m)| {
            p.extension().is_some_and(|e| e == "ts")
                && *m >= w.from
                && *m - Duration::from_secs(SEGMENT_SECS) <= w.until
        })
        .collect();
    hit.sort_by_key(|(_, m)| *m);
    hit.into_iter().map(|(p, _)| p.clone()).collect()
}

/// archive に置く名前 (UTC の `YYYYMMDD-HHMMSS.ts`)
fn archive_name(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // 1970-01-01 からの日数を年月日にする (Howard Hinnant の civil_from_days)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}.ts",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// 自分が付けた名前 (`YYYYMMDD-HHMMSS.ts`) か。archive_dir の中で消してよいのはこの形式だけ
fn is_archive_name(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 18 && b[8] == b'-' && name.ends_with(".ts") && b[..8].iter().chain(&b[9..15]).all(u8::is_ascii_digit)
}

/// keep 本を超える分 (名前の古い順) を返す。自分の名前でないものは数えず、消す対象にもしない
fn expired(names: &[String], keep: usize) -> Vec<String> {
    let mut own: Vec<_> = names.iter().filter(|n| is_archive_name(n)).cloned().collect();
    own.sort();
    let over = own.len().saturating_sub(keep);
    own.truncate(over);
    own
}

/// ディスクの空きが足りるか (取れなければ足りないとみなす)
fn check_space(free_mb: Option<u64>, min_mb: u64) -> anyhow::Result<()> {
    match free_mb {
        Some(f) if f >= min_mb => Ok(()),
        Some(f) => anyhow::bail!("ディスクの空きが {f}MB (min_free_mb = {min_mb}) なので切り出しません"),
        None => anyhow::bail!("ディスクの空きを調べられないので切り出しません"),
    }
}

/// `df -Pk` の出力から、空き (MB) を読む
fn parse_df(out: &str) -> Option<u64> {
    let kb: u64 = out.lines().nth(1)?.split_whitespace().nth(3)?.parse().ok()?;
    Some(kb / 1024)
}

async fn free_mb(dir: &Path) -> Option<u64> {
    let out = tokio::process::Command::new("df")
        .arg("-Pk")
        .arg(dir)
        .output()
        .await
        .ok()?;
    parse_df(&String::from_utf8_lossy(&out.stdout))
}

/// ring の中のファイルと更新時刻
fn list_ring(dir: &Path) -> anyhow::Result<Vec<(PathBuf, SystemTime)>> {
    let mut files = Vec::new();
    for e in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let e = e?;
        if let Ok(m) = e.metadata().and_then(|m| m.modified()) {
            files.push((e.path(), m));
        }
    }
    Ok(files)
}

/// 範囲の分を 1 本にして archive に置き、古いものを消す
async fn cut(cfg: &RecordConfig, ffmpeg: &str, w: Window) -> anyhow::Result<()> {
    check_space(free_mb(Path::new(&cfg.archive_dir)).await, cfg.min_free_mb)?;
    let ring = std::path::absolute(&cfg.ring_dir)?;
    let archive = std::path::absolute(&cfg.archive_dir)?;
    let segments = select_segments(&list_ring(&ring)?, w);
    anyhow::ensure!(!segments.is_empty(), "ring に該当するファイルがありません");
    let name = archive_name(w.until);
    let list = archive.join(format!(".concat-{name}.txt"));
    let body: String = segments.iter().map(|p| format!("file '{}'\n", p.display())).collect();
    std::fs::write(&list, body).with_context(|| format!("writing {}", list.display()))?;
    // 圧縮し直さず (-c copy)、配信の邪魔にならないよう nice にする
    let out = archive.join(&name);
    let status = tokio::process::Command::new("nice")
        .args(["-n", "19", ffmpeg, "-hide_banner", "-loglevel", "error", "-y"])
        .args(["-f", "concat", "-safe", "0", "-i"])
        .arg(&list)
        .args(["-c", "copy"])
        .arg(&out)
        .status()
        .await;
    let _ = std::fs::remove_file(&list);
    let status = status.context("starting ffmpeg (concat)")?;
    if !status.success() {
        let _ = std::fs::remove_file(&out);
        anyhow::bail!("ffmpeg (concat) が失敗しました: {status}");
    }
    tracing::info!(file = %out.display(), segments = segments.len(), "record: 切り出しました");
    let names: Vec<String> = std::fs::read_dir(&archive)?
        .filter_map(|e| Some(e.ok()?.file_name().to_string_lossy().into_owned()))
        .collect();
    for n in expired(&names, cfg.keep) {
        if let Err(e) = std::fs::remove_file(archive.join(&n)) {
            tracing::warn!("record: {n} を消せません: {e}");
        }
    }
    Ok(())
}

async fn run_cut(cfg: &RecordConfig, ffmpeg: &str, w: Window) {
    if let Err(e) = cut(cfg, ffmpeg, w).await {
        tracing::warn!("record: {e:#}");
    }
}

/// 地震の画面への切り替えを見て、切り出しを予約する。calm の送り手が消えても (配信のやり直し)、済ませてから終わる
pub fn spawn(cfg: RecordConfig, ffmpeg: String, mut calm: watch::Receiver<bool>) -> anyhow::Result<()> {
    // ring は ffmpeg の tee が書くが、ディレクトリは作らない (無いと片側が失敗するので、ここで作る)
    for d in [&cfg.ring_dir, &cfg.archive_dir] {
        std::fs::create_dir_all(d).with_context(|| format!("creating {d}"))?;
    }
    tokio::spawn(async move {
        let mut window: Option<Window> = None;
        loop {
            let due = window.map(|w| w.until.duration_since(SystemTime::now()).unwrap_or_default());
            tokio::select! {
                r = calm.changed() => {
                    if r.is_err() {
                        if let (Some(w), Some(d)) = (window, due) {
                            tokio::time::sleep(d).await;
                            run_cut(&cfg, &ffmpeg, w).await;
                        }
                        return;
                    }
                    if !*calm.borrow_and_update() {
                        window = Some(reserve(window, SystemTime::now(), &cfg));
                        tracing::info!(minutes = cfg.after_min, "record: 切り出しを予約しました");
                    }
                }
                _ = tokio::time::sleep(due.unwrap_or_default()), if due.is_some() => {
                    if let Some(w) = window.take() {
                        run_cut(&cfg, &ffmpeg, w).await;
                    }
                }
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn file(n: &str, secs: u64) -> (PathBuf, SystemTime) {
        (PathBuf::from(n), at(secs))
    }

    #[test]
    fn segments_overlapping_the_window_are_picked_in_time_order() {
        let files = [
            file("03.ts", 4000),
            file("00.ts", 1000), // 範囲より前
            file("01.ts", 2000), // from ちょうど
            file("02.ts", 3000),
            file("04.ts", 4060), // 更新時刻は until の後だが、前の 60 秒が範囲にかかる
            file("05.ts", 4120), // 完全に範囲の後
            file("x.txt", 3000), // .ts 以外
        ];
        let w = Window {
            from: at(2000),
            until: at(4000),
        };
        let names: Vec<_> = select_segments(&files, w)
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["01.ts", "02.ts", "03.ts", "04.ts"]);
    }

    #[test]
    fn a_second_quake_extends_the_same_window() {
        let cfg = RecordConfig::default();
        let first = reserve(None, at(10_000), &cfg);
        assert_eq!(first.from, at(10_000 - 300));
        assert_eq!(first.until, at(10_000 + 600));
        let second = reserve(Some(first), at(10_200), &cfg);
        assert_eq!(second.from, first.from);
        assert_eq!(second.until, at(10_200 + 600));
    }

    #[test]
    fn only_own_names_count_and_the_oldest_go_first() {
        let names: Vec<String> = [
            "20260930-140000.ts",
            "20260930-150000.ts",
            "20260930-160000.ts",
            "memo.txt",
            "20260930-1400.ts",
            ".concat-20260930-140000.ts.txt",
            "00.ts",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(expired(&names, 2), ["20260930-140000.ts"]);
        assert!(expired(&names, 3).is_empty());
        assert_eq!(expired(&names, 0).len(), 3);
    }

    #[test]
    fn archive_name_is_utc_and_recognized_as_own() {
        assert_eq!(archive_name(at(1_790_000_000)), "20260921-141320.ts");
        assert_eq!(archive_name(at(951_782_400)), "20000229-000000.ts");
        assert!(is_archive_name(&archive_name(at(1_790_000_000))));
    }

    #[test]
    fn low_or_unknown_free_space_refuses() {
        assert!(check_space(Some(500), 500).is_ok());
        assert!(check_space(Some(499), 500).is_err());
        assert!(check_space(None, 500).is_err());
    }

    #[test]
    fn df_output_is_read() {
        let out =
            "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/sda1 10000000 8700000 1048576 90% /\n";
        assert_eq!(parse_df(out), Some(1024));
        assert_eq!(parse_df("garbage"), None);
    }
}
