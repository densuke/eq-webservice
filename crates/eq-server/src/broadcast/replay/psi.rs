//! e2 の詰まりへの配慮。Linux の /proc/pressure/{io,memory} の full の 60 秒平均が高い間は、描くのも ffmpeg に渡すのも止めて待つ。
//! 10 分以上待っても下がらなければ、あきらめる (作りかけは呼び出し側が消す。R3.3 のキューがやり直す)。Mac では /proc が無く、待たない。
//! deploy/youtube-live-watch.sh の見送りと同じ値 (20%)。

use std::time::{Duration, Instant};

/// full の 60 秒平均がこれ (%) を超えたら待つ
const LIMIT: f64 = 20.0;
/// 見る間隔 (待っている間は、この間隔で見直す)
const CHECK_EVERY: Duration = Duration::from_secs(30);
/// この時間待っても下がらなければあきらめる
const GIVE_UP_AFTER: Duration = Duration::from_secs(600);

/// /proc/pressure/* の文から、full の 60 秒平均 (%) を読む
pub fn full_avg60(text: &str) -> Option<f64> {
    let line = text.lines().find(|l| l.starts_with("full "))?;
    line.split_whitespace()
        .find_map(|w| w.strip_prefix("avg60=")?.parse().ok())
}

/// io か memory のどちらかが、limit (%) を超えて詰まっているか。読めないものは詰まっていないとみなす
pub fn is_congested(io: Option<&str>, memory: Option<&str>, limit: f64) -> bool {
    [io, memory]
        .into_iter()
        .flatten()
        .filter_map(full_avg60)
        .any(|v| v > limit)
}

/// 今、詰まっているか (作る係が見張りで使う。Mac など /proc が無いときは詰まっていない)
pub fn congested_now(limit: f64) -> bool {
    let read = |p: &str| std::fs::read_to_string(p).ok();
    is_congested(
        read("/proc/pressure/io").as_deref(),
        read("/proc/pressure/memory").as_deref(),
        limit,
    )
}

#[derive(Debug, PartialEq)]
pub enum Verdict {
    Go,
    Wait,
    GiveUp,
}

/// 詰まっている状態で waited だけ待った。続けるか、待つか、あきらめるか
pub fn verdict(congested: bool, waited: Duration) -> Verdict {
    match (congested, waited >= GIVE_UP_AFTER) {
        (false, _) => Verdict::Go,
        (true, false) => Verdict::Wait,
        (true, true) => Verdict::GiveUp,
    }
}

/// 30 秒ごとに詰まりを見て、詰まっていれば待つ
#[derive(Default)]
pub struct Throttle {
    checked: Option<Instant>,
}

impl Throttle {
    /// 前に見てから 30 秒たっていなければ、何もせず返る。詰まっていれば、下がるまで待つ
    pub async fn wait(&mut self) -> anyhow::Result<()> {
        if self.checked.is_some_and(|t| t.elapsed() < CHECK_EVERY) {
            return Ok(());
        }
        let started = Instant::now();
        loop {
            let (io, mem) = (
                tokio::fs::read_to_string("/proc/pressure/io").await.ok(),
                tokio::fs::read_to_string("/proc/pressure/memory").await.ok(),
            );
            match verdict(is_congested(io.as_deref(), mem.as_deref(), LIMIT), started.elapsed()) {
                Verdict::Go => break,
                Verdict::Wait => {
                    tracing::warn!("e2 が詰まっているので、待ちます (io・memory の full の 60 秒平均が {LIMIT}% 超)");
                    tokio::time::sleep(CHECK_EVERY).await;
                }
                Verdict::GiveUp => anyhow::bail!(
                    "詰まりが {} 分以上続いたので、あきらめます",
                    GIVE_UP_AFTER.as_secs() / 60
                ),
            }
        }
        self.checked = Some(Instant::now());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDLE: &str =
        "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n";
    const BUSY: &str =
        "some avg10=50.00 avg60=45.10 avg300=9.00 total=1\nfull avg10=30.00 avg60=25.50 avg300=5.00 total=1\n";
    /// some だけが高く、full は低い (全体は止まっていない)
    const SOME_ONLY: &str =
        "some avg10=90.00 avg60=90.00 avg300=9.00 total=1\nfull avg10=1.00 avg60=2.00 avg300=5.00 total=1\n";

    #[test]
    fn reads_the_full_line_sixty_second_average() {
        assert_eq!(full_avg60(BUSY), Some(25.5));
        assert_eq!(full_avg60(SOME_ONLY), Some(2.0));
        assert_eq!(full_avg60("some avg60=9.0\n"), None);
        assert_eq!(full_avg60(""), None);
    }

    #[test]
    fn waits_only_when_io_or_memory_is_over_the_limit() {
        assert!(!is_congested(Some(IDLE), Some(IDLE), LIMIT));
        assert!(is_congested(Some(IDLE), Some(BUSY), LIMIT));
        assert!(is_congested(Some(BUSY), None, LIMIT));
        assert!(!is_congested(Some(SOME_ONLY), Some(SOME_ONLY), LIMIT));
        // Mac など、読めないときは待たない
        assert!(!is_congested(None, None, LIMIT));
        // ちょうど 20% は待たない
        assert!(!is_congested(
            Some("full avg10=0 avg60=20.00 avg300=0 total=0"),
            None,
            LIMIT
        ));
    }

    #[test]
    fn the_limit_is_a_parameter() {
        assert!(!is_congested(Some(BUSY), None, 30.0));
        assert!(is_congested(Some(BUSY), None, 25.0));
    }

    #[test]
    fn gives_up_after_ten_minutes_of_waiting() {
        assert_eq!(verdict(false, Duration::from_secs(9999)), Verdict::Go);
        assert_eq!(verdict(true, Duration::from_secs(599)), Verdict::Wait);
        assert_eq!(verdict(true, Duration::from_secs(600)), Verdict::GiveUp);
    }
}
