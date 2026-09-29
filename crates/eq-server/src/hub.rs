//! 受信したイベントの集約点。重複排除・直近履歴の保持・購読者への配信を行う。

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use eq_core::{Event, EventBody};
use tokio::sync::broadcast;

/// 購読者が処理しきれない場合に溜めておける件数
const CHANNEL_CAPACITY: usize = 256;
/// 重複排除のために覚えておく ID 数
const SEEN_CAPACITY: usize = 4096;

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
        if ev.received_at_ms == 0 {
            ev.received_at_ms = now_ms();
        }
        let ev = Arc::new(ev);
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
                EventBody::Eew(x) if x.event_id == e.event_id => Some(serial(x)),
                _ => None,
            };
            if st.recent.iter().filter_map(same).any(|s| s > serial(e)) {
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

fn serial(e: &eq_core::Eew) -> u32 {
    e.serial.parse().unwrap_or(0)
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
    use eq_core::{Eew, EewDetection, Scale, Tsunami};

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
}
