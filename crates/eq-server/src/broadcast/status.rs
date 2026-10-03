//! 配信の状態の札 (docs/broadcast-status.md)。ライブ配信だけが出す。
//! A. 混雑中: 1 秒ごとに「遅いか」を見て、10 秒続いたら出し、60 秒落ち着いたら消す。
//! B. 途切れた: 起動のとき、前の最後のコマから 30 秒以上あいていたら、起動から 10 分のあいだ出す。
//! 時計と /proc に触れる所は薄くして、判断は時刻と文字を受け取る関数に分けてある。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::psi;
use crate::quake::jst;

/// PSI (io か memory) の full の 10 秒平均 (%) がこれを超えたら遅い
const PSI_LIMIT: f64 = 10.0;
/// コマを送る予定の時刻から、これを超えて遅れたら遅い
const LAG_LIMIT: Duration = Duration::from_secs(2);
/// エンコーダへの 1 コマの書き込みが、これを超えたら遅い
pub const WRITE_LIMIT: Duration = Duration::from_secs(1);
/// 遅い状態がこれ続いたら札を出す
const ON_AFTER_MS: u64 = 10_000;
/// 遅くない状態がこれ続いたら札を消す
const OFF_AFTER_MS: u64 = 60_000;
/// 前の最後のコマから、これを超えてあいていたら途切れたとみなす
const OUTAGE_MIN_MS: u64 = 30_000;
/// 途切れた札を出す長さ (起動から)
const OUTAGE_SHOW_MS: u64 = 600_000;

/// 見張りの間隔
const SAMPLE_EVERY: Duration = Duration::from_secs(1);

/// 配信が途切れていた間 (epoch ミリ秒。最後のコマの時刻から、起動の時刻まで)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outage {
    pub from_ms: u64,
    pub to_ms: u64,
}

/// 画面に出す札
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Notice {
    Busy,
    Outage(Outage),
}

impl Notice {
    pub fn text(&self) -> String {
        match self {
            Notice::Busy => "サーバー混雑中・映像が遅れることがあります".to_string(),
            Notice::Outage(o) => format!("{}〜{} 配信が途切れました", hhmm(o.from_ms), hhmm(o.to_ms)),
        }
    }
}

/// epoch ミリ秒の JST の時刻 (HH:MM)
fn hhmm(ms: u64) -> String {
    jst::format(ms as i64).get(11..16).unwrap_or("--:--").to_string()
}

/// コマの予定からの遅れと、書き込みにかかった時間から、遅いか
pub fn is_slow(lag: Duration, write: Duration) -> bool {
    lag > LAG_LIMIT || write > WRITE_LIMIT
}

/// この 1 秒が遅いか。PSI (読めなければ遅くない) か、コマの遅れ・書き込みの遅れのどれか
pub fn is_busy(io: Option<&str>, memory: Option<&str>, slow: bool) -> bool {
    let pressed = [io, memory]
        .into_iter()
        .flatten()
        .filter_map(psi::full_avg10)
        .any(|v| v > PSI_LIMIT);
    pressed || slow
}

/// 遅い・遅くないの 1 秒ごとの判定から、札を出すか決める (ちらつき止め)。
/// 札なし: 遅い状態が 10 秒続いたら出す。札あり: 遅くない状態が 60 秒続いたら消す。途中で逆が挟まると数え直す
#[derive(Debug, Default)]
pub struct Hysteresis {
    on: bool,
    /// 今の状態と逆の判定が続き始めた時刻
    flip_since: Option<u64>,
}

impl Hysteresis {
    pub fn update(&mut self, now_ms: u64, bad: bool) -> bool {
        // 今の状態を変える側の判定 (札なしなら遅い、札ありなら遅くない)
        if bad == self.on {
            self.flip_since = None;
            return self.on;
        }
        let since = *self.flip_since.get_or_insert(now_ms);
        let hold = if self.on { OFF_AFTER_MS } else { ON_AFTER_MS };
        if now_ms.saturating_sub(since) >= hold {
            self.on = !self.on;
            self.flip_since = None;
        }
        self.on
    }
}

/// 前の最後のコマ (epoch ミリ秒) と起動の時刻から、途切れていたか
pub fn outage_of(last_frame_ms: Option<u64>, start_ms: u64) -> Option<Outage> {
    let from_ms = last_frame_ms?;
    (start_ms.saturating_sub(from_ms) > OUTAGE_MIN_MS).then_some(Outage {
        from_ms,
        to_ms: start_ms,
    })
}

/// 今出す札。混雑中を先にし、無ければ途切れた札 (起動から 10 分のあいだ)
pub fn notice_of(busy: bool, outage: Option<Outage>, now_ms: u64) -> Option<Notice> {
    if busy {
        return Some(Notice::Busy);
    }
    outage
        .filter(|o| now_ms.saturating_sub(o.to_ms) < OUTAGE_SHOW_MS)
        .map(Notice::Outage)
}

/// コマを送る側 (配信の本線) が書き残す、遅さの印。コマごとの仕事は、時計を 2 回読んで旗を立てるだけ
pub struct Load {
    origin: Instant,
    /// 前の見張りから今までに、遅いコマがあったか
    slow: AtomicBool,
    /// 書き込み中なら、始めた時刻 (origin からのミリ秒 + 1。0 は書いていない)
    writing_since: AtomicU64,
}

impl Load {
    pub fn new() -> Arc<Load> {
        Arc::new(Load {
            origin: Instant::now(),
            slow: AtomicBool::new(false),
            writing_since: AtomicU64::new(0),
        })
    }

    /// コマの書き込みを始める
    pub fn begin_write(&self) {
        self.begin_write_at(self.origin.elapsed());
    }

    /// 書き込みが終わった。lag は、コマの予定の時刻からの遅れ
    pub fn end_write(&self, lag: Duration) {
        self.end_write_at(self.origin.elapsed(), lag);
    }

    fn begin_write_at(&self, at: Duration) {
        self.writing_since.store(at.as_millis() as u64 + 1, Ordering::Relaxed);
    }

    fn end_write_at(&self, at: Duration, lag: Duration) {
        let since = self.writing_since.swap(0, Ordering::Relaxed);
        let wrote = at.saturating_sub(Duration::from_millis(since.saturating_sub(1)));
        if since != 0 && is_slow(lag, wrote) || since == 0 && is_slow(lag, Duration::ZERO) {
            self.slow.store(true, Ordering::Relaxed);
        }
    }

    /// 前の見張りから今までに遅いコマがあったか (旗は下ろす)。書き込みが終わらないまま 1 秒たっていても遅い
    fn take_slow_at(&self, at: Duration) -> bool {
        let flagged = self.slow.swap(false, Ordering::Relaxed);
        let since = self.writing_since.load(Ordering::Relaxed);
        let stuck = since != 0 && at.saturating_sub(Duration::from_millis(since - 1)) > WRITE_LIMIT;
        flagged || stuck
    }
}

/// 配信 (ライブ) が native の描画に渡す、状態の札の材料。再現動画はこれを持たない (札を出さない)
pub struct Feed {
    pub busy: watch::Receiver<bool>,
    pub outage: Option<Outage>,
}

impl Feed {
    /// 今出す札 (描画のたびに呼ぶ。watch を 1 回読むだけ)
    pub fn notice(&self, now_ms: u64) -> Option<Notice> {
        notice_of(*self.busy.borrow(), self.outage, now_ms)
    }
}

/// 1 秒ごとの見張り: 遅いかを判定して、札を出すかを決める
pub struct Monitor {
    load: Arc<Load>,
    hysteresis: Hysteresis,
}

impl Monitor {
    pub fn new(load: Arc<Load>) -> Self {
        Monitor {
            load,
            hysteresis: Hysteresis::default(),
        }
    }

    /// 1 回見張る。now_ms は epoch ミリ秒、at は Load の origin からの経過、io・memory は PSI の文
    fn sample(&mut self, now_ms: u64, at: Duration, io: Option<&str>, memory: Option<&str>) -> bool {
        let busy = is_busy(io, memory, self.load.take_slow_at(at));
        self.hysteresis.update(now_ms, busy)
    }
}

/// 1 秒ごとに見張って、混雑中かを watch で渡す (PSI はここでだけ読む)。受け手が全部いなくなったら終わる
pub fn spawn(load: Arc<Load>) -> (watch::Receiver<bool>, JoinHandle<()>) {
    let (tx, rx) = watch::channel(false);
    let task = tokio::spawn(async move {
        let origin = load.origin;
        let mut monitor = Monitor::new(load);
        let mut tick = tokio::time::interval(SAMPLE_EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let read = |p: &str| std::fs::read_to_string(p).ok();
            let (io, mem) = (read("/proc/pressure/io"), read("/proc/pressure/memory"));
            let busy = monitor.sample(
                super::calm_state::now_ms(),
                origin.elapsed(),
                io.as_deref(),
                mem.as_deref(),
            );
            tx.send_if_modified(|b| std::mem::replace(b, busy) != busy);
            if tx.is_closed() {
                return;
            }
        }
    });
    (rx, task)
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDLE: &str =
        "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n";
    const HIGH: &str =
        "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=10.01 avg60=0.00 avg300=0.00 total=0\n";
    const EDGE: &str = "full avg10=10.00 avg60=99.00 avg300=0.00 total=0\n";
    /// 60 秒平均だけ高い (今は落ち着いている)
    const OLD_BUSY: &str = "full avg10=0.50 avg60=80.00 avg300=0.00 total=0\n";

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn slow_when_the_frame_lags_over_2s_or_the_write_takes_over_1s() {
        assert!(!is_slow(secs(2), secs(1)));
        assert!(is_slow(Duration::from_millis(2001), Duration::ZERO));
        assert!(is_slow(Duration::ZERO, Duration::from_millis(1001)));
        assert!(!is_slow(Duration::ZERO, Duration::ZERO));
    }

    #[test]
    fn busy_when_psi_ten_second_average_is_over_10_or_slow() {
        assert!(!is_busy(Some(IDLE), Some(IDLE), false));
        assert!(is_busy(Some(IDLE), Some(HIGH), false));
        assert!(is_busy(Some(HIGH), None, false));
        // ちょうど 10.0 は遅くない。60 秒平均は見ない
        assert!(!is_busy(Some(EDGE), None, false));
        assert!(!is_busy(Some(OLD_BUSY), None, false));
        // /proc が読めない (Mac) ときは、PSI では遅くならない。コマの遅れだけが効く
        assert!(!is_busy(None, None, false));
        assert!(is_busy(None, None, true));
    }

    /// 1 秒ごとに判定を入れて、(秒, 札あり) の変わり目を返す
    fn run(h: &mut Hysteresis, from: u64, bads: &[bool]) -> Vec<(u64, bool)> {
        let mut changes = Vec::new();
        let mut last = h.on;
        for (i, &b) in bads.iter().enumerate() {
            let t = from + i as u64;
            let on = h.update(t * 1000, b);
            if on != last {
                changes.push((t, on));
                last = on;
            }
        }
        changes
    }

    #[test]
    fn turns_on_after_ten_seconds_of_bad_and_off_after_sixty_of_clear() {
        let mut h = Hysteresis::default();
        // 0 秒から 20 秒間遅い。10 秒続いた 10 秒の時点で出る。そのあと落ち着いて、60 秒続いた時点で消える
        let mut seq = vec![true; 20];
        seq.extend(vec![false; 70]);
        assert_eq!(run(&mut h, 0, &seq), vec![(10, true), (80, false)]);
    }

    #[test]
    fn a_short_bad_spell_does_not_turn_it_on() {
        let mut h = Hysteresis::default();
        let mut seq = vec![true; 9];
        seq.extend(vec![false; 30]);
        assert_eq!(run(&mut h, 0, &seq), vec![]);
        assert!(!h.update(40_000, false));
    }

    #[test]
    fn flapping_does_not_toggle_it() {
        let mut h = Hysteresis::default();
        // 出す側: 9 秒遅い・1 秒落ち着く、を繰り返しても出ない
        let flap: Vec<bool> = (0..100).map(|i| i % 10 != 9).collect();
        assert_eq!(run(&mut h, 0, &flap), vec![]);
        // 出したあと: 59 秒落ち着いて 1 秒遅い、を繰り返しても消えない
        let mut h = Hysteresis::default();
        assert_eq!(run(&mut h, 0, &[true; 11]), vec![(10, true)]);
        let flap: Vec<bool> = (0..300).map(|i| i % 60 == 59).collect();
        assert_eq!(run(&mut h, 11, &flap), vec![]);
        // 遅いのがやんで 60 秒たてば消える
        assert_eq!(run(&mut h, 311, &[false; 61]), vec![(371, false)]);
    }

    #[test]
    fn a_missing_or_short_gap_is_not_an_outage() {
        let start = 10_000_000;
        assert_eq!(outage_of(None, start), None);
        assert_eq!(outage_of(Some(start - 30_000), start), None);
        assert_eq!(
            outage_of(Some(start - 30_001), start),
            Some(Outage {
                from_ms: start - 30_001,
                to_ms: start
            })
        );
        // 時計が戻っていても (last が未来) 途切れとはしない
        assert_eq!(outage_of(Some(start + 5), start), None);
    }

    #[test]
    fn the_outage_notice_lasts_ten_minutes_from_the_start_and_busy_comes_first() {
        let o = Outage {
            from_ms: 1_000,
            to_ms: 100_000,
        };
        assert_eq!(notice_of(false, None, 0), None);
        assert_eq!(notice_of(true, None, 0), Some(Notice::Busy));
        assert_eq!(notice_of(false, Some(o), 100_000), Some(Notice::Outage(o)));
        assert_eq!(notice_of(false, Some(o), 100_000 + 599_999), Some(Notice::Outage(o)));
        assert_eq!(notice_of(false, Some(o), 100_000 + 600_000), None);
        // 混雑中が先。消えたあと、10 分が残っていれば途切れた札に戻る
        assert_eq!(notice_of(true, Some(o), 200_000), Some(Notice::Busy));
        assert_eq!(notice_of(false, Some(o), 200_000), Some(Notice::Outage(o)));
    }

    #[test]
    fn a_slow_frame_sets_the_flag_once_and_a_stuck_write_counts_while_it_runs() {
        let l = Load::new();
        let at = |ms| Duration::from_millis(ms);
        // 速いコマは旗を立てない
        l.begin_write_at(at(100));
        l.end_write_at(at(150), at(10));
        assert!(!l.take_slow_at(at(200)));
        // 予定から 2 秒を超えて遅れたコマ
        l.begin_write_at(at(300));
        l.end_write_at(at(310), at(2500));
        assert!(l.take_slow_at(at(400)));
        // 旗は 1 回読むと下りる
        assert!(!l.take_slow_at(at(500)));
        // 書き込みに 1 秒を超えかかったコマ
        l.begin_write_at(at(1000));
        l.end_write_at(at(2200), at(0));
        assert!(l.take_slow_at(at(2300)));
        // 書き込みが終わらないままのとき: 1 秒まではまだ、超えたら遅い
        l.begin_write_at(at(5000));
        assert!(!l.take_slow_at(at(5900)));
        assert!(l.take_slow_at(at(6100)));
        // 終われば (遅れ・長さは今の時刻で決まる) 旗が立つ
        l.end_write_at(at(9000), at(0));
        assert!(l.take_slow_at(at(9100)));
    }

    #[test]
    fn the_monitor_turns_on_after_ten_bad_seconds_and_back_off_after_sixty_clear() {
        let l = Load::new();
        let mut m = Monitor::new(l.clone());
        let mut on_at = None;
        let mut off_at = None;
        for t in 0..100u64 {
            // 最初の 20 秒は毎秒遅いコマがある。そのあとは落ち着く
            if t < 20 {
                l.end_write_at(Duration::from_secs(t), Duration::from_secs(3));
            }
            let on = m.sample(t * 1000, Duration::from_secs(t), Some(IDLE), None);
            if on && on_at.is_none() {
                on_at = Some(t);
            }
            if !on && on_at.is_some() && off_at.is_none() {
                off_at = Some(t);
            }
        }
        assert_eq!((on_at, off_at), (Some(10), Some(80)));
        // PSI だけでも効く
        let mut m = Monitor::new(Load::new());
        assert!((0..=10)
            .map(|t| m.sample(t * 1000, Duration::from_secs(t), None, Some(HIGH)))
            .last()
            .unwrap());
    }

    /// 2026-10 の JST の日・時・分の epoch ミリ秒
    fn jst_ms(day: u32, h: u32, m: u32) -> u64 {
        let s = jst::parse_ms(&format!("2026/10/{day:02} {h:02}:{m:02}:00")).unwrap();
        s as u64
    }

    #[test]
    fn the_texts_are_in_jst_and_cross_midnight() {
        assert_eq!(Notice::Busy.text(), "サーバー混雑中・映像が遅れることがあります");
        let o = Outage {
            from_ms: jst_ms(3, 9, 5),
            to_ms: jst_ms(3, 9, 42),
        };
        assert_eq!(Notice::Outage(o).text(), "09:05〜09:42 配信が途切れました");
        let o = Outage {
            from_ms: jst_ms(3, 23, 50),
            to_ms: jst_ms(4, 0, 12),
        };
        assert_eq!(Notice::Outage(o).text(), "23:50〜00:12 配信が途切れました");
    }
}
