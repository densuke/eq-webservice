//! 受信したイベントの集約点。重複排除・直近履歴の保持・購読者への配信を行う。

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::quake::{Event, EventBody};
use tokio::sync::broadcast;

/// 購読者が処理しきれない場合に溜めておける件数
const CHANNEL_CAPACITY: usize = 256;
/// 重複排除のために覚えておく ID 数
const SEEN_CAPACITY: usize = 4096;
/// 読み上げ (tts) のために覚えておく受信済みイベントの数。EEW の古い報は直近履歴から消えるので、こちらで引けるようにする
const PUBLISHED_CAPACITY: usize = 256;
/// 取り消し済みとして覚えておく地震 (EEW の event_id) の数。直近履歴から押し出されても後の報で戻さないため
const CANCELLED_CAPACITY: usize = 1024;

pub struct Hub {
    tx: broadcast::Sender<Arc<Event>>,
    state: Mutex<State>,
    recent_capacity: usize,
}

struct State {
    recent: VecDeque<Arc<Event>>,
    /// 最新の津波予報。直近履歴から押し出されても、解除されるまでブラウザに渡し続ける
    tsunami: Option<Arc<Event>>,
    seen: HashSet<String>,
    seen_order: VecDeque<String>,
    /// 受信した順のイベント (直近履歴と違い、新しい報で置き換えない)
    published: VecDeque<Arc<Event>>,
    /// 取り消し済みの地震 (EEW の event_id)。これ以降の報は配信も保持もしない
    cancelled: HashSet<String>,
    cancelled_order: VecDeque<String>,
}

impl Hub {
    pub fn new(recent_capacity: usize) -> Arc<Hub> {
        let (tx, _) = broadcast::channel(CHANNEL_CAPACITY);
        Arc::new(Hub {
            tx,
            state: Mutex::new(State {
                recent: VecDeque::new(),
                tsunami: None,
                seen: HashSet::new(),
                seen_order: VecDeque::new(),
                published: VecDeque::new(),
                cancelled: HashSet::new(),
                cancelled_order: VecDeque::new(),
            }),
            recent_capacity,
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Event>> {
        self.tx.subscribe()
    }

    /// 新しいイベントを配信する。既に受け取った ID なら false。
    pub fn publish(&self, ev: Event) -> bool {
        match self.remember(ev) {
            Some(ev) => {
                // 購読者がいないときの Err は無視してよい
                let _ = self.tx.send(ev);
                true
            }
            None => false,
        }
    }

    /// 起動時の履歴取り込み。直近履歴には入れるが、配信 (Discord 通知など) はしない。
    pub fn seed(&self, events: Vec<Event>) -> Vec<Arc<Event>> {
        events.into_iter().filter_map(|ev| self.remember(ev)).collect()
    }

    /// 直近のイベント (古い順)。最新の津波予報が押し出されていれば先頭に足す
    pub fn recent(&self) -> Vec<Arc<Event>> {
        let st = self.state.lock().unwrap();
        let pinned = st.tsunami.as_ref().filter(|t| !st.recent.iter().any(|e| e.id == t.id));
        pinned.into_iter().chain(st.recent.iter()).cloned().collect()
    }

    /// 知っている報すべて (届いた順)。直近履歴と受信した報の保持分を id で重複なくまとめる
    pub fn snapshot(&self) -> Vec<Arc<Event>> {
        let st = self.state.lock().unwrap();
        let mut all: Vec<Arc<Event>> = st.published.iter().cloned().collect();
        // 受信保持分から漏れた直近履歴・津波予報を足す
        for e in st.recent.iter().chain(st.tsunami.as_ref()) {
            if !all.iter().any(|a| a.id == e.id) {
                all.push(e.clone());
            }
        }
        // 安定ソートなので同時刻は並びを保つ
        all.sort_by_key(|e| e.received_at_ms);
        all
    }

    /// id でイベントを探す (範囲は `recent()` と同じ)
    pub fn get(&self, id: &str) -> Option<Arc<Event>> {
        let st = self.state.lock().unwrap();
        let found = st.recent.iter().find(|e| e.id == id);
        found
            .or(st.tsunami.as_ref().filter(|t| t.id == id))
            .or_else(|| st.published.iter().find(|e| e.id == id))
            .cloned()
    }

    fn remember(&self, mut ev: Event) -> Option<Arc<Event>> {
        let mut st = self.state.lock().unwrap();
        if !st.seen.insert(ev.id.clone()) {
            return None;
        }
        st.seen_order.push_back(ev.id.clone());
        if st.seen_order.len() > SEEN_CAPACITY {
            if let Some(old) = st.seen_order.pop_front() {
                st.seen.remove(&old);
            }
        }
        // 一度取り消された地震は、後の報 (別の配信元の取り消し・送り直し・遅れた古い報) で戻さない。
        // 気象庁の取り消しはその地震について最終。2026-10-16 12:38 千島列島 (M9.9 と過大推定) では
        // 取り消しが P2P の第 2 報だけで届き、そのあと Wolfx が第 42 報を送り直してきた。戻す作りだと震度 7 の予報が復活する
        // event_id が無い (P2P で issue が欠けると空になる) 報は、どの地震か分からないので覚えも遮断もしない
        let eew = match &ev.body {
            EventBody::Eew(e) if !e.event_id.trim().is_empty() => Some(e),
            _ => None,
        };
        if let Some(e) = eew {
            if st.cancelled.contains(&e.event_id) {
                return None;
            }
            if e.cancelled {
                st.cancelled.insert(e.event_id.clone());
                st.cancelled_order.push_back(e.event_id.clone());
                if st.cancelled_order.len() > CANCELLED_CAPACITY {
                    if let Some(old) = st.cancelled_order.pop_front() {
                        st.cancelled.remove(&old);
                    }
                }
            }
        }
        if ev.received_at_ms == 0 {
            ev.received_at_ms = now_ms();
        }
        let ev = Arc::new(ev);
        st.published.push_back(ev.clone());
        if st.published.len() > PUBLISHED_CAPACITY {
            st.published.pop_front();
        }
        if matches!(ev.body, EventBody::Tsunami(_))
            && st
                .tsunami
                .as_ref()
                .is_none_or(|t| t.issued_at_ms() <= ev.issued_at_ms())
        {
            st.tsunami = Some(ev.clone());
        }
        // 同じ地震の EEW は最新の報だけを直近履歴に残す (予報は 1 地震で何報も出るため)。
        // 遅れて届いた古い報は配信はするが、直近履歴には入れない
        if let EventBody::Eew(e) = &ev.body {
            let same = |r: &Arc<Event>| match &r.body {
                EventBody::Eew(x) if x.event_id == e.event_id => Some(rank(x)),
                _ => None,
            };
            if st.recent.iter().filter_map(same).any(|s| s > rank(e)) {
                return Some(ev);
            }
            st.recent.retain(|r| same(r).is_none());
        }
        // 地震感知情報は同じ揺れ (開始時刻) の最新の評価だけを直近履歴に残す
        if let EventBody::Userquake(u) = &ev.body {
            st.recent
                .retain(|r| !matches!(&r.body, EventBody::Userquake(x) if x.started_at == u.started_at));
        }
        st.recent.push_back(ev.clone());
        while st.recent.len() > self.recent_capacity {
            st.recent.pop_front();
        }
        Some(ev)
    }
}

/// 新しさの順位。取り消しは serial に関係なく最後 (配信元ごとに serial が違い、P2P の取り消しは小さい番号で届く)。
/// 取り消しのあとの報は `cancelled` で弾くので、ここでは取り消しが直近履歴の報を置き換えられればよい
fn rank(e: &crate::quake::Eew) -> u32 {
    if e.cancelled {
        u32::MAX
    } else {
        e.serial.parse().unwrap_or(0)
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quake::{Eew, EewDetection, Scale, Tsunami};

    fn ev(id: &str) -> Event {
        Event {
            id: id.into(),
            source: "test".into(),
            received_at_ms: 0,
            body: EventBody::EewDetection(EewDetection {
                detection_type: "Full".into(),
            }),
        }
    }

    #[tokio::test]
    async fn dedupes_and_broadcasts() {
        let hub = Hub::new(2);
        let mut rx = hub.subscribe();
        assert!(hub.publish(ev("a")));
        assert!(!hub.publish(ev("a")));
        assert!(hub.publish(ev("b")));
        assert!(hub.publish(ev("c")));
        assert_eq!(rx.recv().await.unwrap().id, "a");
        assert_eq!(rx.recv().await.unwrap().id, "b");
        let recent: Vec<_> = hub.recent().iter().map(|e| e.id.clone()).collect();
        assert_eq!(recent, ["b", "c"]);
    }

    fn tsunami(id: &str, issued_at: &str) -> Event {
        Event {
            id: id.into(),
            source: "test".into(),
            received_at_ms: 0,
            body: EventBody::Tsunami(Tsunami {
                cancelled: false,
                issued_at: issued_at.into(),
                areas: vec![],
            }),
        }
    }

    fn eew(id: &str, event_id: &str, serial: &str) -> Event {
        Event {
            id: id.into(),
            source: "test".into(),
            received_at_ms: 0,
            body: EventBody::Eew(Eew {
                event_id: event_id.into(),
                serial: serial.into(),
                cancelled: false,
                test: false,
                warning: false,
                issued_at: "2026/09/29 04:45:10".into(),
                origin_time: None,
                origin_time_ms: None,
                hypocenter: None,
                areas: vec![],
                pref_max: vec![],
                max_scale: Scale::S4,
            }),
        }
    }

    #[tokio::test]
    async fn keeps_only_the_latest_eew_report_per_earthquake() {
        let hub = Hub::new(10);
        let mut rx = hub.subscribe();
        hub.publish(eew("a1", "A", "1"));
        hub.publish(ev("x"));
        hub.publish(eew("a3", "A", "3"));
        // 遅れて届いた古い報は配信するが直近履歴には入れない
        assert!(hub.publish(eew("a2", "A", "2")));
        hub.publish(eew("b1", "B", "1"));
        let recent: Vec<_> = hub.recent().iter().map(|e| e.id.clone()).collect();
        assert_eq!(recent, ["x", "a3", "b1"]);
        let mut sent = vec![];
        while let Ok(e) = rx.try_recv() {
            sent.push(e.id.clone());
        }
        assert_eq!(sent, ["a1", "x", "a3", "a2", "b1"]);
    }

    #[test]
    fn a_cancel_replaces_the_report_even_with_a_lower_serial() {
        // P2P の取り消しは第 2 報、Wolfx は第 42 報まで進んでいる (2026-10-16 千島列島)
        let hub = Hub::new(10);
        hub.publish(eew("w42", "A", "42"));
        let mut cancel = eew("p2", "A", "2");
        if let EventBody::Eew(e) = &mut cancel.body {
            e.cancelled = true;
        }
        hub.publish(cancel);
        let mut rx = hub.subscribe();
        // 取り消しのあとの報は配信もしない (読み上げや通知が再び鳴らないように)
        assert!(!hub.publish(eew("w43", "A", "43")));
        assert!(rx.try_recv().is_err());
        let recent: Vec<_> = hub.recent().iter().map(|e| e.id.clone()).collect();
        assert_eq!(recent, ["p2"], "取り消しのあとの報も取り消しを上書きしない");
    }

    fn cancel(id: &str, event_id: &str, serial: &str) -> Event {
        let mut ev = eew(id, event_id, serial);
        if let EventBody::Eew(e) = &mut ev.body {
            e.cancelled = true;
        }
        ev
    }

    fn ids(events: &[Arc<Event>]) -> Vec<String> {
        events.iter().map(|e| e.id.clone()).collect()
    }

    #[test]
    fn a_resent_report_after_the_cancel_does_not_revive_the_earthquake() {
        // 2026-10-16 12:38 千島列島の順序。取り消しは P2P の第 2 報だけで、
        // そのあと Wolfx が第 42 報を送り直してきた (実際は古さで弾かれたが、別 id で届いても戻さない)
        let hub = Hub::new(10);
        for (id, serial) in [("p1", "1"), ("p2", "2"), ("w40", "40"), ("w41", "41"), ("w42", "42")] {
            assert!(hub.publish(eew(id, "A", serial)));
        }
        assert!(hub.publish(cancel("p2-cancel", "A", "2")));
        let mut rx = hub.subscribe();
        assert!(!hub.publish(eew("w42-resent", "A", "42")));
        assert!(rx.try_recv().is_err());
        assert_eq!(ids(&hub.recent()), ["p2-cancel"]);
        assert!(
            !ids(&hub.snapshot()).contains(&"w42-resent".to_string()),
            "取り消し後の報は受信保持にも残さない"
        );
    }

    #[test]
    fn a_cancel_from_both_sources_is_delivered_once() {
        let hub = Hub::new(10);
        hub.publish(eew("w42", "A", "42"));
        let mut rx = hub.subscribe();
        assert!(hub.publish(cancel("w43", "A", "43")));
        assert!(!hub.publish(cancel("p2", "A", "2")));
        assert_eq!(rx.try_recv().unwrap().id, "w43");
        assert!(rx.try_recv().is_err());
        assert_eq!(ids(&hub.recent()), ["w43"]);
    }

    #[test]
    fn a_cancel_pushed_out_of_recent_still_blocks_later_reports() {
        let hub = Hub::new(2);
        hub.publish(eew("w42", "A", "42"));
        hub.publish(cancel("p2", "A", "2"));
        hub.publish(ev("x"));
        hub.publish(ev("y"));
        assert_eq!(ids(&hub.recent()), ["x", "y"]);
        assert!(!hub.publish(eew("w43", "A", "43")));
        assert_eq!(ids(&hub.recent()), ["x", "y"]);
    }

    #[test]
    fn an_old_report_arriving_after_the_cancel_is_dropped() {
        let hub = Hub::new(10);
        hub.publish(cancel("p2", "A", "2"));
        assert!(!hub.publish(eew("w1", "A", "1")));
        assert_eq!(ids(&hub.recent()), ["p2"]);
    }

    #[test]
    fn a_cancel_without_event_id_does_not_block_other_eews() {
        // P2P の event_id は欠けると空になる。空どうしを同じ地震とみなして遮断しない
        let hub = Hub::new(10);
        assert!(hub.publish(cancel("c", "", "1")));
        assert!(hub.publish(eew("other", "", "1")));
        assert!(hub.publish(cancel("c-blank", " ", "1")));
        assert!(hub.publish(eew("blank", " ", "1")));
    }

    #[test]
    fn cancelled_ids_are_forgotten_oldest_first() {
        let hub = Hub::new(10);
        for i in 0..=CANCELLED_CAPACITY {
            hub.publish(cancel(&format!("c{i}"), &format!("E{i}"), "1"));
        }
        // 最も古い取り消しは忘れ、新しいものは覚えている
        assert!(hub.publish(eew("old", "E0", "2")));
        assert!(!hub.publish(eew("new", &format!("E{CANCELLED_CAPACITY}"), "2")));
    }

    #[test]
    fn keeps_latest_tsunami_after_it_is_pushed_out() {
        let hub = Hub::new(2);
        hub.publish(tsunami("t2", "2026/09/28 12:10:00"));
        // 遅れて届いた古い予報では置き換えない
        hub.publish(tsunami("t1", "2026/09/28 12:00:00"));
        hub.publish(ev("a"));
        hub.publish(ev("b"));
        let recent: Vec<_> = hub.recent().iter().map(|e| e.id.clone()).collect();
        assert_eq!(recent, ["t2", "a", "b"]);
    }

    #[tokio::test]
    async fn seed_does_not_broadcast() {
        let hub = Hub::new(10);
        let mut rx = hub.subscribe();
        hub.seed(vec![ev("x")]);
        assert!(!hub.publish(ev("x")));
        assert!(rx.try_recv().is_err());
        assert_eq!(hub.recent().len(), 1);
    }

    #[test]
    fn get_finds_published_event_by_id() {
        let hub = Hub::new(10);
        hub.publish(ev("a"));
        hub.publish(ev("b"));
        assert_eq!(hub.get("b").unwrap().id, "b");
    }

    #[test]
    fn get_finds_an_eew_report_superseded_by_a_later_serial() {
        // 読み上げは第 1 報の id で取りに来る。その間に第 2 報が届いて直近履歴から消えても見つかること
        let hub = Hub::new(10);
        hub.publish(eew("e1", "Q", "1"));
        hub.publish(eew("e2", "Q", "2"));
        assert_eq!(hub.get("e1").unwrap().id, "e1");
    }

    #[test]
    fn snapshot_keeps_superseded_eew_in_arrival_order() {
        let hub = Hub::new(10);
        hub.publish(eew("e1", "Q", "1"));
        hub.publish(ev("x"));
        hub.publish(eew("e2", "Q", "2"));
        // e1 は直近履歴から消えているが、snapshot には残る
        let ids: Vec<_> = hub.snapshot().iter().map(|e| e.id.clone()).collect();
        assert_eq!(ids, ["e1", "x", "e2"]);
    }

    #[test]
    fn published_capacity_is_256() {
        assert_eq!(PUBLISHED_CAPACITY, 256);
    }

    #[test]
    fn get_returns_none_for_unknown_id() {
        let hub = Hub::new(10);
        hub.publish(ev("a"));
        assert!(hub.get("zzz").is_none());
    }
}
