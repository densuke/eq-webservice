//! 受信したイベントの集約点。重複排除・直近履歴の保持・購読者への配信を行う。

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use eq_core::Event;
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

    /// 直近のイベント (古い順)
    pub fn recent(&self) -> Vec<Arc<Event>> {
        self.state.lock().unwrap().recent.iter().cloned().collect()
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
        st.recent.push_back(ev.clone());
        while st.recent.len() > self.recent_capacity {
            st.recent.pop_front();
        }
        Some(ev)
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
    use eq_core::{EewDetection, EventBody};

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
