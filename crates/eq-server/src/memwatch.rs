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

/// "key value" の行から key の値を取る (memory.stat・memory.events)
fn field(text: &str, key: &str) -> Option<u64> {
    text.lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix(' ')?.trim().parse().ok())
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Reading {
    /// memory.current: ページキャッシュ (file) を含む。上限に張り付いても、カーネルが回収するので落ちるとは限らない
    current: u64,
    peak: Option<u64>,
    max: Option<u64>,
    /// memory.stat の anon (プロセスが持つメモリ。回収できないので OOM の本当の材料) と file (ページキャッシュ)
    anon: Option<u64>,
    file: Option<u64>,
    /// memory.events の max (上限に当たって回収した回数) と oom_kill
    events_max: Option<u64>,
    oom_kill: Option<u64>,
}

impl Reading {
    /// anon (読めなければ current) が上限の 80% を超えているか (上限なしなら false)。
    /// ページキャッシュは回収されるので、警告の材料にしない
    fn is_high(&self) -> bool {
        let used = self.anon.unwrap_or(self.current);
        self.max
            .is_some_and(|m| m > 0 && used as u128 * 100 > m as u128 * WARN_PERCENT as u128)
    }

    fn line(&self) -> String {
        let mb = |b: u64| b / (1024 * 1024);
        let opt = |v: Option<u64>| v.map_or("-".to_string(), |b| mb(b).to_string());
        let n = |v: Option<u64>| v.map_or("-".to_string(), |b| b.to_string());
        format!(
            "memory: current_mb={} anon_mb={} file_mb={} peak_mb={} max_mb={} events_max={} oom_kill={}",
            mb(self.current),
            opt(self.anon),
            opt(self.file),
            opt(self.peak),
            opt(self.max),
            n(self.events_max),
            n(self.oom_kill)
        )
    }
}

/// cgroup のディレクトリから読む。memory.current が読めなければ None (ほかは古いカーネルに無いことがある)
fn read(dir: &Path) -> Option<Reading> {
    let text = |name: &str| std::fs::read_to_string(dir.join(name)).ok();
    let get = |name: &str| text(name).and_then(|s| parse_bytes(&s));
    let (stat, events) = (text("memory.stat"), text("memory.events"));
    Some(Reading {
        current: get("memory.current")?,
        peak: get("memory.peak"),
        max: get("memory.max"),
        anon: stat.as_deref().and_then(|s| field(s, "anon")),
        file: stat.as_deref().and_then(|s| field(s, "file")),
        events_max: events.as_deref().and_then(|s| field(s, "max")),
        oom_kill: events.as_deref().and_then(|s| field(s, "oom_kill")),
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
                tracing::warn!(target: "memwatch", "{} (anon が上限の {WARN_PERCENT}% 超)", r.line());
            } else {
                tracing::info!(target: "memwatch", "{}", r.line());
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
    fn warns_on_anon_not_on_page_cache() {
        let r = |current, anon, max| {
            Reading {
                current,
                anon,
                max,
                ..Default::default()
            }
            .is_high()
        };
        assert!(!r(204 * MB, None, Some(256 * MB)));
        assert!(r(206 * MB, None, Some(256 * MB)));
        // ページキャッシュで current が上限に張り付いても、anon が低ければ警告しない
        assert!(!r(335 * MB, Some(142 * MB), Some(335 * MB)));
        assert!(r(335 * MB, Some(280 * MB), Some(335 * MB)));
        // ちょうど 80% は警告しない
        assert!(!r(80, None, Some(100)));
        assert!(r(81, None, Some(100)));
        assert!(!r(u64::MAX, None, None));
        assert!(!r(1, None, Some(0)));
    }

    #[test]
    fn picks_fields_by_exact_key() {
        let stat = "anon 100\nfile 200\nfile_mapped 5\nanon_thp 7\n";
        assert_eq!(field(stat, "anon"), Some(100));
        assert_eq!(field(stat, "file"), Some(200));
        assert_eq!(field(stat, "shmem"), None);
        let ev = "low 0\nhigh 0\nmax 58\noom 0\noom_kill 0\n";
        assert_eq!(field(ev, "max"), Some(58));
        assert_eq!(field(ev, "oom_kill"), Some(0));
    }

    #[test]
    fn the_line_shows_megabytes_and_dashes_for_missing_values() {
        let full = Reading {
            current: 335 * MB,
            peak: Some(335 * MB),
            max: Some(335 * MB),
            anon: Some(142 * MB),
            file: Some(188 * MB),
            events_max: Some(58),
            oom_kill: Some(0),
        };
        assert_eq!(
            full.line(),
            "memory: current_mb=335 anon_mb=142 file_mb=188 peak_mb=335 max_mb=335 events_max=58 oom_kill=0"
        );
        let bare = Reading {
            current: 5 * MB,
            ..Default::default()
        };
        assert_eq!(
            bare.line(),
            "memory: current_mb=5 anon_mb=- file_mb=- peak_mb=- max_mb=- events_max=- oom_kill=-"
        );
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
                ..Default::default()
            })
        );
        std::fs::write(d.path().join("memory.peak"), "2097152\n").unwrap();
        std::fs::write(d.path().join("memory.stat"), "anon 3\nfile 4\n").unwrap();
        std::fs::write(d.path().join("memory.events"), "max 5\noom_kill 6\n").unwrap();
        let r = read(d.path()).unwrap();
        assert_eq!(
            (r.peak, r.anon, r.file, r.events_max, r.oom_kill),
            (Some(2 * MB), Some(3), Some(4), Some(5), Some(6))
        );
    }
}
