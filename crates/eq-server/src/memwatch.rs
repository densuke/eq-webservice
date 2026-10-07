//! 自分のプロセスの cgroup (v2) のメモリを 1 分ごとにログへ出す (Issue #196)。
//! 最大値がいつ出たかを後から特定するのが目的。MemoryMax (memory.max) の 80% を超えたら WARN。
//! cgroup v2 が読めない環境 (macOS・テスト) では何もしない。子プロセス (ffmpeg) も cgroup に含まれる。

use std::path::{Path, PathBuf};
use std::time::Duration;

const EVERY: Duration = Duration::from_secs(60);
/// memory.max に対する警告の割合 (%)
const WARN_PERCENT: u64 = 80;
const CGROUP_ROOT: &str = "/sys/fs/cgroup";

/// /proc/self/cgroup の内容から cgroup v2 のパス ("0::/..." の行) を取り出す
fn cgroup_v2_path(proc_self_cgroup: &str) -> Option<&str> {
    proc_self_cgroup
        .lines()
        .find_map(|l| l.strip_prefix("0::"))
        .map(str::trim)
}

/// memory.current などの 1 行のバイト数。"max" (無制限) や読めないものは None
fn parse_bytes(s: &str) -> Option<u64> {
    s.trim().parse().ok()
}

#[derive(Debug, PartialEq, Eq)]
struct Reading {
    current: u64,
    peak: Option<u64>,
    max: Option<u64>,
}

impl Reading {
    /// 現在値が上限の 80% を超えているか (上限なしなら false)
    fn is_high(&self) -> bool {
        self.max
            .is_some_and(|m| m > 0 && self.current as u128 * 100 > m as u128 * WARN_PERCENT as u128)
    }

    fn line(&self) -> String {
        let mb = |b: u64| b / (1024 * 1024);
        let opt = |v: Option<u64>| v.map_or("-".to_string(), |b| mb(b).to_string());
        format!(
            "memory: current_mb={} peak_mb={} max_mb={}",
            mb(self.current),
            opt(self.peak),
            opt(self.max)
        )
    }
}

/// cgroup のディレクトリから読む。memory.current が読めなければ None (peak は古いカーネルに無い)
fn read(dir: &Path) -> Option<Reading> {
    let get = |name: &str| {
        std::fs::read_to_string(dir.join(name))
            .ok()
            .and_then(|s| parse_bytes(&s))
    };
    Some(Reading {
        current: get("memory.current")?,
        peak: get("memory.peak"),
        max: get("memory.max"),
    })
}

fn own_cgroup_dir() -> Option<PathBuf> {
    let text = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    let rel = cgroup_v2_path(&text)?;
    Some(Path::new(CGROUP_ROOT).join(rel.trim_start_matches('/')))
}

/// 1 分ごとに記録するタスクを起こす。cgroup v2 が読めなければ何もしない
pub fn spawn() {
    let Some(dir) = own_cgroup_dir().filter(|d| read(d).is_some()) else {
        return;
    };
    tokio::spawn(async move {
        let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + EVERY, EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let Some(r) = read(&dir) else { continue };
            if r.is_high() {
                tracing::warn!("{} (上限の {WARN_PERCENT}% 超)", r.line());
            } else {
                tracing::info!("{}", r.line());
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1024 * 1024;

    #[test]
    fn finds_the_v2_path_and_ignores_v1_lines() {
        assert_eq!(
            cgroup_v2_path("0::/user.slice/user-1000.slice/user@1000.service/app.slice/eq-broadcast.service\n"),
            Some("/user.slice/user-1000.slice/user@1000.service/app.slice/eq-broadcast.service")
        );
        assert_eq!(cgroup_v2_path("12:memory:/foo\n11:cpu:/foo\n"), None);
        assert_eq!(cgroup_v2_path("12:memory:/foo\n0::/bar\n"), Some("/bar"));
        assert_eq!(cgroup_v2_path(""), None);
    }

    #[test]
    fn parses_bytes_and_treats_max_as_unlimited() {
        assert_eq!(parse_bytes("268435456\n"), Some(256 * MB));
        assert_eq!(parse_bytes("max\n"), None);
        assert_eq!(parse_bytes("garbage"), None);
    }

    #[test]
    fn warns_only_above_80_percent_of_max() {
        let r = |current, max| {
            Reading {
                current,
                peak: None,
                max,
            }
            .is_high()
        };
        assert!(!r(204 * MB, Some(256 * MB)));
        assert!(r(206 * MB, Some(256 * MB)));
        // ちょうど 80% は警告しない
        assert!(!r(80, Some(100)));
        assert!(r(81, Some(100)));
        assert!(!r(u64::MAX, None));
        assert!(!r(1, Some(0)));
    }

    #[test]
    fn the_line_shows_megabytes_and_dashes_for_missing_values() {
        let full = Reading {
            current: 130 * MB,
            peak: Some(301 * MB),
            max: Some(320 * MB),
        };
        assert_eq!(full.line(), "memory: current_mb=130 peak_mb=301 max_mb=320");
        let bare = Reading {
            current: 5 * MB,
            peak: None,
            max: None,
        };
        assert_eq!(bare.line(), "memory: current_mb=5 peak_mb=- max_mb=-");
    }

    #[test]
    fn reads_the_files_in_a_cgroup_directory() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(read(d.path()), None);
        std::fs::write(d.path().join("memory.current"), "1048576\n").unwrap();
        std::fs::write(d.path().join("memory.max"), "max\n").unwrap();
        assert_eq!(
            read(d.path()),
            Some(Reading {
                current: MB,
                peak: None,
                max: None
            })
        );
        std::fs::write(d.path().join("memory.peak"), "2097152\n").unwrap();
        assert_eq!(read(d.path()).unwrap().peak, Some(2 * MB));
    }
}
