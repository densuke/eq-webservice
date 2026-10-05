//! 配信の状態の札 (docs/broadcast-status.md)。ライブ配信だけが出す。
//! A. 混雑中: 1 秒ごとに「遅いか」を見て、10 秒続いたら出し、60 秒落ち着いたら消す。
//! B. 途切れた: 起動のとき、前の最後のコマから 30 秒以上あいていたら、起動から 10 分のあいだ出す。
//! 時計と /proc に触れる所は薄くして、判断は時刻と文字を受け取る関数に分けてある。

use std::sync::atomic::{AtomicU64, Ordering};
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

/// 前の見張りから今までの、コマの遅れと書き込みの長さの最大
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Window {
    /// コマの予定の時刻からの遅れの最大
    pub lag: Duration,
    /// 1 コマの書き込みの長さの最大 (終わっていない書き込みは、今までの長さ)
    pub write: Duration,
    /// 書き込みが終わらないまま WRITE_LIMIT を超えていたか
    pub stuck: bool,
}

/// 見張りの記録 (1 秒の分でも、遅い状態が続いた間の分でも。値は最大)。遅いかの判定はここで決める
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Worst {
    pub window: Window,
    /// PSI io の full の 10 秒平均 (%)。読めなければ 0
    pub io: f64,
    /// PSI memory の full の 10 秒平均 (%)。読めなければ 0
    pub memory: f64,
}

impl Worst {
    pub fn of(window: Window, io: Option<&str>, memory: Option<&str>) -> Self {
        let avg = |t: Option<&str>| t.and_then(psi::full_avg10).unwrap_or(0.0);
        Worst {
            window,
            io: avg(io),
            memory: avg(memory),
        }
    }

    pub fn merge(self, o: Worst) -> Worst {
        Worst {
            window: Window {
                lag: self.window.lag.max(o.window.lag),
                write: self.window.write.max(o.window.write),
                stuck: self.window.stuck || o.window.stuck,
            },
            io: self.io.max(o.io),
            memory: self.memory.max(o.memory),
        }
    }

    /// 遅いと判定した理由 (無ければ遅くない)。PSI は読めなければ遅くない
    pub fn reasons(&self) -> Vec<&'static str> {
        [
            ("frame_lag", self.window.lag > LAG_LIMIT),
            ("write_slow", self.window.write > WRITE_LIMIT),
            ("write_stuck", self.window.stuck),
            ("psi_io", self.io > PSI_LIMIT),
            ("psi_memory", self.memory > PSI_LIMIT),
        ]
        .into_iter()
        .filter_map(|(name, hit)| hit.then_some(name))
        .collect()
    }

    pub fn is_busy(&self) -> bool {
        !self.reasons().is_empty()
    }

    /// ログの 1 行 (札の出入りと、混雑中の 1 分ごと)
    pub fn line(&self, head: &str) -> String {
        let reasons = self.reasons();
        format!(
            "{head} reasons={} lag_ms={} write_ms={} write_stuck={} psi_io_full_avg10={:.1} psi_memory_full_avg10={:.1}",
            if reasons.is_empty() { "-".to_string() } else { reasons.join(",") },
            self.window.lag.as_millis(),
            self.window.write.as_millis(),
            self.window.stuck,
            self.io,
            self.memory,
        )
    }
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

/// コマを送る側 (配信の本線) が書き残す、遅さの印。コマごとの仕事は、時計を 2 回読んで最大を更新するだけ
pub struct Load {
    origin: Instant,
    /// 前の見張りから今までの、コマの遅れの最大 (マイクロ秒)
    max_lag_us: AtomicU64,
    /// 前の見張りから今までの、書き込みの長さの最大 (マイクロ秒)
    max_write_us: AtomicU64,
    /// 書き込み中なら、始めた時刻 (origin からのミリ秒 + 1。0 は書いていない)
    writing_since: AtomicU64,
}

impl Load {
    pub fn new() -> Arc<Load> {
        Arc::new(Load {
            origin: Instant::now(),
            max_lag_us: AtomicU64::new(0),
            max_write_us: AtomicU64::new(0),
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
        self.max_lag_us.fetch_max(lag.as_micros() as u64, Ordering::Relaxed);
        if since != 0 {
            let wrote = at.saturating_sub(Duration::from_millis(since - 1));
            self.max_write_us.fetch_max(wrote.as_micros() as u64, Ordering::Relaxed);
        }
    }

    /// 前の見張りから今までの最大 (最大は 0 に戻す)。書き込みが終わらないまま 1 秒たっていても遅い
    fn take_window_at(&self, at: Duration) -> Window {
        let lag = Duration::from_micros(self.max_lag_us.swap(0, Ordering::Relaxed));
        let done = Duration::from_micros(self.max_write_us.swap(0, Ordering::Relaxed));
        let running = match self.writing_since.load(Ordering::Relaxed) {
            0 => Duration::ZERO,
            since => at.saturating_sub(Duration::from_millis(since - 1)),
        };
        Window {
            lag,
            write: done.max(running),
            stuck: running > WRITE_LIMIT,
        }
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

/// 混雑中のあいだ、まとめのログを出す間隔
const SUMMARY_EVERY_MS: u64 = 60_000;

/// 1 秒ごとの見張り: 遅いかを判定して、札を出すかを決める。札の出入りと、出ている間の 1 分ごとのまとめを、ログの文で返す
pub struct Monitor {
    load: Arc<Load>,
    hysteresis: Hysteresis,
    on: bool,
    /// 札が出る前: 遅い状態が続いた間の最大。札が出ている間: 前のまとめからの最大
    worst: Worst,
    /// 札が出ている間: 次のまとめを出す時刻 (epoch ミリ秒)
    next_summary_ms: u64,
}

impl Monitor {
    pub fn new(load: Arc<Load>) -> Self {
        Monitor {
            load,
            hysteresis: Hysteresis::default(),
            on: false,
            worst: Worst::default(),
            next_summary_ms: 0,
        }
    }

    /// 1 回見張る。now_ms は epoch ミリ秒、at は Load の origin からの経過、io・memory は PSI の文。
    /// 返すのは、札を出すか・出すログの文 (札が出ない間は何も出さない)
    fn sample(&mut self, now_ms: u64, at: Duration, io: Option<&str>, memory: Option<&str>) -> (bool, Option<String>) {
        let now = Worst::of(self.load.take_window_at(at), io, memory);
        let on = self.hysteresis.update(now_ms, now.is_busy());
        let was = std::mem::replace(&mut self.on, on);
        // 札が出る前は、遅い状態が途切れたら数え直す (ヒステリシスと同じ)
        self.worst = if on || now.is_busy() {
            self.worst.merge(now)
        } else {
            Worst::default()
        };
        let head = match (was, on) {
            (false, true) => {
                self.next_summary_ms = now_ms + SUMMARY_EVERY_MS;
                "broadcast: busy chip ON (slow for 10s+)"
            }
            (true, false) => "broadcast: busy chip OFF (clear for 60s+, worst since last summary)",
            (true, true) if now_ms >= self.next_summary_ms => {
                self.next_summary_ms = now_ms + SUMMARY_EVERY_MS;
                "broadcast: busy chip still ON, last minute"
            }
            _ => return (on, None),
        };
        let line = self.worst.line(head);
        self.worst = Worst::default();
        (on, Some(line))
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
            let (busy, line) = monitor.sample(
                super::calm_state::now_ms(),
                origin.elapsed(),
                io.as_deref(),
                mem.as_deref(),
            );
            if let Some(line) = line {
                tracing::info!("{line}");
            }
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

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn win(lag: u64, write: u64, stuck: bool) -> Window {
        Window {
            lag: ms(lag),
            write: ms(write),
            stuck,
        }
    }

    #[test]
    fn slow_when_the_frame_lags_over_2s_or_the_write_takes_over_1s() {
        let r = |w| Worst::of(w, None, None).reasons();
        assert!(r(win(2000, 1000, false)).is_empty());
        assert_eq!(r(win(2001, 0, false)), vec!["frame_lag"]);
        assert_eq!(r(win(0, 1001, false)), vec!["write_slow"]);
        assert_eq!(r(win(0, 0, true)), vec!["write_stuck"]);
        assert!(r(Window::default()).is_empty());
    }

    #[test]
    fn busy_when_psi_ten_second_average_is_over_10_or_slow() {
        let idle = Window::default();
        let r = |io, mem, w| Worst::of(w, io, mem).reasons();
        assert!(r(Some(IDLE), Some(IDLE), idle).is_empty());
        assert_eq!(r(Some(IDLE), Some(HIGH), idle), vec!["psi_memory"]);
        assert_eq!(r(Some(HIGH), None, idle), vec!["psi_io"]);
        // ちょうど 10.0 は遅くない。60 秒平均は見ない
        assert!(r(Some(EDGE), None, idle).is_empty());
        assert!(r(Some(OLD_BUSY), None, idle).is_empty());
        // /proc が読めない (Mac) ときは、PSI では遅くならない。コマの遅れだけが効く
        assert!(r(None, None, idle).is_empty());
        assert_eq!(
            r(None, Some(HIGH), win(3000, 0, false)),
            vec!["frame_lag", "psi_memory"]
        );
    }

    #[test]
    fn the_log_line_names_the_reasons_and_the_worst_values() {
        let w = Worst::of(win(3100, 120, false), Some(HIGH), Some(IDLE));
        assert_eq!(
            w.line("head"),
            "head reasons=frame_lag,psi_io lag_ms=3100 write_ms=120 write_stuck=false psi_io_full_avg10=10.0 psi_memory_full_avg10=0.0"
        );
        assert!(Worst::default().line("h").contains("reasons=-"));
        // 合わせると、どちらの最大も残る
        let m = Worst::of(win(100, 2000, true), None, Some(HIGH)).merge(w);
        assert_eq!(
            (m.window.lag, m.window.write, m.window.stuck),
            (ms(3100), ms(2000), true)
        );
        assert_eq!(
            m.reasons(),
            vec!["frame_lag", "write_slow", "write_stuck", "psi_io", "psi_memory"]
        );
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
    fn the_load_keeps_the_max_since_the_last_take_and_a_stuck_write_counts_while_it_runs() {
        let l = Load::new();
        // 速いコマ
        l.begin_write_at(ms(100));
        l.end_write_at(ms(150), ms(10));
        assert_eq!(l.take_window_at(ms(200)), win(10, 50, false));
        // 予定から 2.5 秒遅れたコマと、速いコマ。最大が残る
        l.begin_write_at(ms(300));
        l.end_write_at(ms(310), ms(2500));
        l.begin_write_at(ms(320));
        l.end_write_at(ms(330), ms(5));
        assert_eq!(l.take_window_at(ms(400)), win(2500, 10, false));
        // 1 回読むと 0 に戻る
        assert_eq!(l.take_window_at(ms(500)), Window::default());
        // 書き込みに 1.2 秒かかったコマ
        l.begin_write_at(ms(1000));
        l.end_write_at(ms(2200), ms(0));
        assert_eq!(l.take_window_at(ms(2300)), win(0, 1200, false));
        // 書き込みが終わらないままのとき: 1 秒まではまだ、超えたら詰まり
        l.begin_write_at(ms(5000));
        assert_eq!(l.take_window_at(ms(5900)), win(0, 900, false));
        assert_eq!(l.take_window_at(ms(6100)), win(0, 1100, true));
        // 終われば長さが残る
        l.end_write_at(ms(9000), ms(0));
        assert_eq!(l.take_window_at(ms(9100)), win(0, 4000, false));
    }

    #[test]
    fn the_monitor_turns_on_after_ten_bad_seconds_and_back_off_after_sixty_clear() {
        let l = Load::new();
        let mut m = Monitor::new(l.clone());
        let mut on_at = None;
        let mut off_at = None;
        // (秒, 札の出入りと、まとめのログ)
        let mut lines = Vec::new();
        for t in 0..200u64 {
            // 最初の 100 秒は毎秒遅いコマがある (遅れは秒ごとに違う)。そのあとは落ち着く
            if t < 100 {
                l.end_write_at(Duration::from_secs(t), ms(3000 + t));
            }
            let (on, line) = m.sample(t * 1000, Duration::from_secs(t), Some(IDLE), None);
            if let Some(line) = line {
                lines.push((t, line));
            }
            if on && on_at.is_none() {
                on_at = Some(t);
            }
            if !on && on_at.is_some() && off_at.is_none() {
                off_at = Some(t);
            }
        }
        assert_eq!((on_at, off_at), (Some(10), Some(160)));
        let ts: Vec<u64> = lines.iter().map(|(t, _)| *t).collect();
        // 出たとき、出ている間の 1 分ごと、消えたとき。落ち着いているあいだは何も出さない
        assert_eq!(ts, vec![10, 70, 130, 160]);
        // 出たときは、遅い状態が続いた間 (0〜10 秒) の最大
        assert!(lines[0].1.contains("ON") && lines[0].1.contains("lag_ms=3010 "));
        assert!(lines[1].1.contains("still ON") && lines[1].1.contains("lag_ms=3070 "));
        assert!(lines[3].1.contains("OFF"));
        // PSI だけでも効く
        let mut m = Monitor::new(Load::new());
        let out: Vec<_> = (0..=10)
            .map(|t| m.sample(t * 1000, Duration::from_secs(t), None, Some(HIGH)))
            .collect();
        assert!(out[10].0);
        assert!(out[10].1.as_deref().unwrap().contains("reasons=psi_memory"));
        assert!(out[..10].iter().all(|(on, l)| !on && l.is_none()));
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
