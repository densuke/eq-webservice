//! 月間の合成文字数の予算管理 (docs/tts.md の S4)。
//!
//! `{cache_dir}/usage.json` に `{"month":"YYYY-MM","chars":N}` を保存する。
//! 月は UTC。月が変わると 0 に戻る。合成の前に `check`、成功後に `record` を呼ぶ。
//! ファイルは一時ファイル + rename で原子的に書く。欠損・破損は 0 から始めて警告を出す。

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// 予算超過で合成を断ったことを表す。
#[derive(Debug, PartialEq, Eq)]
pub struct BudgetExceeded;

#[derive(Debug, Serialize, Deserialize)]
struct Usage {
    month: String,
    chars: usize,
}

pub struct Budget {
    path: PathBuf,
    limit: usize,
    usage: Mutex<Usage>,
}

impl Budget {
    /// usage.json を読む。欠損・破損なら 0 から始める (警告ログ)。
    pub fn load(path: impl Into<PathBuf>, limit: usize) -> Budget {
        let path = path.into();
        let usage = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!(path = %path.display(), error = %e, "usage.json が壊れている。0 から始める");
                Usage {
                    month: String::new(),
                    chars: 0,
                }
            }),
            Err(_) => Usage {
                month: String::new(),
                chars: 0,
            },
        };
        Budget {
            path,
            limit,
            usage: Mutex::new(usage),
        }
    }

    /// `chars` 文字を足しても上限を超えないか。超えるなら Err。
    pub fn check(&self, chars: usize, now: SystemTime) -> Result<(), BudgetExceeded> {
        if self.used(now).saturating_add(chars) > self.limit {
            Err(BudgetExceeded)
        } else {
            Ok(())
        }
    }

    /// 合成に成功した文字数を加算して保存する。
    pub fn record(&self, chars: usize, now: SystemTime) -> anyhow::Result<()> {
        let month = month_utc(now);
        let mut u = self.usage.lock().unwrap_or_else(|e| e.into_inner());
        if u.month != month {
            u.month = month;
            u.chars = 0;
        }
        u.chars = u.chars.saturating_add(chars);
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut tmp = self.path.clone().into_os_string();
        tmp.push(".tmp");
        std::fs::write(&tmp, serde_json::to_vec(&*u)?)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    /// 今月の使用済み文字数 (月が変わっていれば 0)。
    pub fn used(&self, now: SystemTime) -> usize {
        let u = self.usage.lock().unwrap_or_else(|e| e.into_inner());
        if u.month == month_utc(now) {
            u.chars
        } else {
            0
        }
    }
}

/// UTC の年月 "YYYY-MM"。
fn month_utc(now: SystemTime) -> String {
    let secs = match now.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    };
    let (y, m, _) = crate::quake::jst::civil_from_days(secs.div_euclid(86_400));
    format!("{y:04}-{m:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    const OCT_END: u64 = 1793491199; // 2026-10-31T23:59:59Z
    const NOV_START: u64 = 1793491200; // 2026-11-01T00:00:00Z

    #[test]
    fn month_utc_epoch() {
        assert_eq!(month_utc(at(0)), "1970-01");
    }

    #[test]
    fn month_utc_leap_day() {
        assert_eq!(month_utc(at(1709164800)), "2024-02"); // 2024-02-29
    }

    #[test]
    fn month_utc_month_boundary() {
        assert_eq!(month_utc(at(OCT_END)), "2026-10");
        assert_eq!(month_utc(at(NOV_START)), "2026-11");
    }

    #[test]
    fn month_utc_year_boundary() {
        assert_eq!(month_utc(at(1798761599)), "2026-12"); // 2026-12-31T23:59:59Z
        assert_eq!(month_utc(at(1798761600)), "2027-01"); // 2027-01-01T00:00:00Z
    }

    #[test]
    fn fresh_budget_allows_up_to_limit() {
        let dir = tempfile::tempdir().unwrap();
        let b = Budget::load(dir.path().join("usage.json"), 10);
        assert_eq!(b.check(10, at(NOV_START)), Ok(()));
        assert_eq!(b.check(11, at(NOV_START)), Err(BudgetExceeded));
    }

    #[test]
    fn record_then_check_respects_limit() {
        let dir = tempfile::tempdir().unwrap();
        let b = Budget::load(dir.path().join("usage.json"), 10);
        b.record(7, at(NOV_START)).unwrap();
        assert_eq!(b.check(3, at(NOV_START)), Ok(()));
        assert_eq!(b.check(4, at(NOV_START)), Err(BudgetExceeded));
    }

    #[test]
    fn usage_survives_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.json");
        Budget::load(&path, 10).record(6, at(NOV_START)).unwrap();
        let b2 = Budget::load(&path, 10);
        assert_eq!(b2.used(at(NOV_START)), 6);
    }

    #[test]
    fn month_rollover_resets_usage() {
        let dir = tempfile::tempdir().unwrap();
        let b = Budget::load(dir.path().join("usage.json"), 10);
        b.record(5, at(OCT_END)).unwrap();
        assert_eq!(b.used(at(OCT_END)), 5);
        assert_eq!(b.used(at(NOV_START)), 0);
        assert_eq!(b.check(10, at(NOV_START)), Ok(()));
    }

    #[test]
    fn corrupt_file_starts_from_zero() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.json");
        std::fs::write(&path, "{not json").unwrap();
        let b = Budget::load(&path, 10);
        assert_eq!(b.used(at(NOV_START)), 0);
    }

    #[test]
    fn missing_file_starts_from_zero() {
        let dir = tempfile::tempdir().unwrap();
        let b = Budget::load(dir.path().join("usage.json"), 10);
        assert_eq!(b.used(at(NOV_START)), 0);
    }

    #[test]
    fn record_writes_json_with_month_and_chars() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.json");
        Budget::load(&path, 10).record(4, at(NOV_START)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["month"], "2026-11");
        assert_eq!(v["chars"], 4);
    }
}
