//! 月間の合成文字数の予算管理 (docs/tts.md の S4)。
//!
//! `{cache_dir}/usage.json` に `{"month":"YYYY-MM","chars":N}` を保存する。
//! 月は UTC。月が変わると 0 に戻る。合成の前に `reserve` で枠を予約し (使用済み + 予約中を同じロックで確認)、
//! 成功したら `Reservation::commit`、失敗・時間切れ・キャンセルなら Drop で返す (S-04)。
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

/// 使用済み (usage) と、合成中でまだ確定していない予約の文字数。同じロックで守る
struct State {
    usage: Usage,
    reserved: usize,
}

pub struct Budget {
    path: PathBuf,
    limit: usize,
    state: Mutex<State>,
}

/// 予約した文字数。commit しないまま Drop (失敗・キャンセル) すると返却される。
#[must_use = "予約は合成が終わるまで持つ。すぐ捨てると返却される"]
pub struct Reservation<'a> {
    budget: &'a Budget,
    chars: usize,
    settled: bool,
}

impl Reservation<'_> {
    /// 合成に成功した。予約を使用済みに変えて保存する。
    pub fn commit(mut self, now: SystemTime) -> anyhow::Result<()> {
        self.settled = true;
        self.budget.settle(self.chars, Some(now))
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if !self.settled {
            let _ = self.budget.settle(self.chars, None);
        }
    }
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
            state: Mutex::new(State { usage, reserved: 0 }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `chars` 文字を予約する。使用済み + 予約中 + chars が上限を超えるなら Err。
    pub fn reserve(&self, chars: usize, now: SystemTime) -> Result<Reservation<'_>, BudgetExceeded> {
        let mut st = self.lock();
        let used = if st.usage.month == month_utc(now) {
            st.usage.chars
        } else {
            0
        };
        if used.saturating_add(st.reserved).saturating_add(chars) > self.limit {
            return Err(BudgetExceeded);
        }
        st.reserved += chars;
        Ok(Reservation {
            budget: self,
            chars,
            settled: false,
        })
    }

    /// 予約を解く。`commit` が Some(now) なら使用済みに加算して保存する。
    fn settle(&self, chars: usize, commit: Option<SystemTime>) -> anyhow::Result<()> {
        let mut st = self.lock();
        st.reserved = st.reserved.saturating_sub(chars);
        let Some(now) = commit else { return Ok(()) };
        let month = month_utc(now);
        if st.usage.month != month {
            st.usage.month = month;
            st.usage.chars = 0;
        }
        st.usage.chars = st.usage.chars.saturating_add(chars);
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut tmp = self.path.clone().into_os_string();
        tmp.push(".tmp");
        std::fs::write(&tmp, serde_json::to_vec(&st.usage)?)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    /// 今月の使用済み文字数 (月が変わっていれば 0)。予約中は含まない。テストで確かめるためだけに使う
    #[cfg(test)]
    pub fn used(&self, now: SystemTime) -> usize {
        let st = self.lock();
        if st.usage.month == month_utc(now) {
            st.usage.chars
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

    /// 予約して確定する
    fn spend(b: &Budget, chars: usize, now: SystemTime) {
        b.reserve(chars, now).unwrap().commit(now).unwrap();
    }

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
        assert!(b.reserve(10, at(NOV_START)).is_ok());
        assert!(b.reserve(11, at(NOV_START)).is_err());
    }

    #[test]
    fn spend_then_reserve_respects_limit() {
        let dir = tempfile::tempdir().unwrap();
        let b = Budget::load(dir.path().join("usage.json"), 10);
        spend(&b, 7, at(NOV_START));
        assert!(b.reserve(3, at(NOV_START)).is_ok());
        assert!(b.reserve(4, at(NOV_START)).is_err());
    }

    #[test]
    fn outstanding_reservation_counts_against_the_limit() {
        // 監査 S-04: 上限 3 に 3 文字を 2 本同時に頼むと、1 本だけ通る
        let dir = tempfile::tempdir().unwrap();
        let b = Budget::load(dir.path().join("usage.json"), 3);
        let first = b.reserve(3, at(NOV_START)).unwrap();
        assert!(b.reserve(3, at(NOV_START)).is_err());
        drop(first);
    }

    #[test]
    fn dropping_a_reservation_returns_it_without_using_it() {
        let dir = tempfile::tempdir().unwrap();
        let b = Budget::load(dir.path().join("usage.json"), 3);
        drop(b.reserve(3, at(NOV_START)).unwrap());
        assert_eq!(b.used(at(NOV_START)), 0);
        assert!(b.reserve(3, at(NOV_START)).is_ok());
    }

    #[test]
    fn commit_moves_the_reservation_to_used() {
        let dir = tempfile::tempdir().unwrap();
        let b = Budget::load(dir.path().join("usage.json"), 3);
        spend(&b, 3, at(NOV_START));
        assert_eq!(b.used(at(NOV_START)), 3);
        assert!(b.reserve(1, at(NOV_START)).is_err());
    }

    #[test]
    fn usage_survives_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.json");
        spend(&Budget::load(&path, 10), 6, at(NOV_START));
        let b2 = Budget::load(&path, 10);
        assert_eq!(b2.used(at(NOV_START)), 6);
    }

    #[test]
    fn month_rollover_resets_usage() {
        let dir = tempfile::tempdir().unwrap();
        let b = Budget::load(dir.path().join("usage.json"), 10);
        spend(&b, 5, at(OCT_END));
        assert_eq!(b.used(at(OCT_END)), 5);
        assert_eq!(b.used(at(NOV_START)), 0);
        assert!(b.reserve(10, at(NOV_START)).is_ok());
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
    fn commit_writes_json_with_month_and_chars() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.json");
        spend(&Budget::load(&path, 10), 4, at(NOV_START));
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["month"], "2026-11");
        assert_eq!(v["chars"], 4);
    }
}
