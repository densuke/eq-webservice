//! WebSocket (`/ws`) の接続の守り。IP ごとの同時接続数・接続頻度と、Origin の検証。
//! 判定は純粋な関数 (`IpState::admit`、`origin_allowed`) に切り出してある。

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct WsLimits {
    /// 1 つの IP から同時につなげる数
    pub max_per_ip: usize,
    /// 1 つの IP が 1 分間につなげる回数 (固定窓)
    pub connects_per_min: u32,
    /// ping を送る間隔
    pub ping_interval: Duration,
    /// クライアントから何も届かない (pong が無い) まま待つ時間。超えたら切る
    pub pong_timeout: Duration,
    /// 1 回の送信にかけてよい時間。詰まったクライアントは切る
    pub send_timeout: Duration,
}

impl WsLimits {
    pub fn new(max_per_ip: usize, connects_per_min: u32) -> Self {
        WsLimits {
            max_per_ip,
            connects_per_min,
            ping_interval: Duration::from_secs(30),
            pong_timeout: Duration::from_secs(90),
            send_timeout: Duration::from_secs(10),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Denied {
    TooManyConnections,
    TooFrequent,
}

const WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug, Default)]
struct IpState {
    active: usize,
    window_start: Option<Instant>,
    in_window: u32,
}

impl IpState {
    /// 接続を 1 つ受けてよいか。受けるなら数を進める (純粋: 時刻は引数)
    fn admit(&mut self, now: Instant, l: &WsLimits) -> Result<(), Denied> {
        if self.active >= l.max_per_ip {
            return Err(Denied::TooManyConnections);
        }
        match self.window_start {
            Some(s) if now.duration_since(s) < WINDOW => {}
            _ => {
                self.window_start = Some(now);
                self.in_window = 0;
            }
        }
        if self.in_window >= l.connects_per_min {
            return Err(Denied::TooFrequent);
        }
        self.in_window += 1;
        self.active += 1;
        Ok(())
    }
}

pub struct WsGuard {
    pub limits: WsLimits,
    pub trusted_proxies: Vec<IpAddr>,
    allowed_origins: Vec<String>,
    ips: Mutex<IpTable>,
}

/// 覚えておく IP の数の上限。超えたら、新しい IP は記録を作らず断る
const MAX_IPS: usize = 4096;
/// 古い記録を片付ける間隔
const PRUNE_EVERY: Duration = Duration::from_secs(10);

#[derive(Default)]
struct IpTable {
    map: HashMap<IpAddr, IpState>,
    last_prune: Option<Instant>,
}

/// 制限を数える単位。IPv6 は /64 (1 契約で丸ごと持てるので) に丸め、IPv4-mapped は IPv4 として扱う
pub fn rate_key(ip: IpAddr) -> IpAddr {
    match ip.to_canonical() {
        IpAddr::V6(v6) => IpAddr::V6((u128::from(v6) & (u128::MAX << 64)).into()),
        v4 => v4,
    }
}

/// 持っている間、その IP の同時接続を 1 つ使う
pub struct IpPermit {
    guard: Arc<WsGuard>,
    ip: IpAddr,
}

impl Drop for IpPermit {
    fn drop(&mut self) {
        let mut t = self.guard.ips.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = t.map.get_mut(&self.ip) {
            s.active = s.active.saturating_sub(1);
        }
    }
}

impl WsGuard {
    pub fn new(limits: WsLimits, trusted_proxies: Vec<IpAddr>, allowed_origins: Vec<String>) -> Arc<Self> {
        Arc::new(WsGuard {
            limits,
            trusted_proxies,
            allowed_origins,
            ips: Mutex::new(IpTable::default()),
        })
    }

    pub fn acquire(self: &Arc<Self>, ip: IpAddr) -> Result<IpPermit, Denied> {
        self.acquire_at(ip, Instant::now())
    }

    fn acquire_at(self: &Arc<Self>, ip: IpAddr, now: Instant) -> Result<IpPermit, Denied> {
        let ip = rate_key(ip);
        let mut t = self.ips.lock().unwrap_or_else(|e| e.into_inner());
        // 古い記録 (つながっておらず窓も過ぎた IP) は一定間隔で片付ける
        if t.last_prune.is_none_or(|p| now.duration_since(p) >= PRUNE_EVERY) {
            t.last_prune = Some(now);
            t.map
                .retain(|_, s| s.active > 0 || s.window_start.is_some_and(|w| now.duration_since(w) < WINDOW));
        }
        if !t.map.contains_key(&ip) {
            if t.map.len() >= MAX_IPS {
                return Err(Denied::TooFrequent);
            }
            // 断るときは記録を残さない
            let mut fresh = IpState::default();
            fresh.admit(now, &self.limits)?;
            t.map.insert(ip, fresh);
        } else if let Some(s) = t.map.get_mut(&ip) {
            s.admit(now, &self.limits)?;
        }
        Ok(IpPermit {
            guard: self.clone(),
            ip,
        })
    }

    #[cfg(test)]
    fn tracked(&self) -> usize {
        self.ips.lock().unwrap().map.len()
    }

    pub fn origin_ok(&self, origin: Option<&str>, host: Option<&str>) -> bool {
        origin_allowed(origin, host, &self.allowed_origins)
    }
}

/// Origin の検証。Origin が無い接続 (非ブラウザ) は通す。
/// あるときは、Host と同じ場所の origin か、許可リストにあるものだけ通す
pub fn origin_allowed(origin: Option<&str>, host: Option<&str>, allowed: &[String]) -> bool {
    let Some(origin) = origin else { return true };
    let origin = origin.trim().trim_end_matches('/');
    if allowed
        .iter()
        .any(|a| a.trim().trim_end_matches('/').eq_ignore_ascii_case(origin))
    {
        return true;
    }
    let authority = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"));
    matches!((authority, host), (Some(a), Some(h)) if !a.is_empty() && a.eq_ignore_ascii_case(h.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(per_ip: usize, per_min: u32) -> WsLimits {
        WsLimits::new(per_ip, per_min)
    }

    #[test]
    fn concurrent_limit_per_ip() {
        let l = limits(2, 100);
        let mut s = IpState::default();
        let t = Instant::now();
        assert!(s.admit(t, &l).is_ok());
        assert!(s.admit(t, &l).is_ok());
        assert_eq!(s.admit(t, &l), Err(Denied::TooManyConnections));
        s.active -= 1;
        assert!(s.admit(t, &l).is_ok());
    }

    #[test]
    fn rate_limit_resets_after_the_window() {
        let l = limits(100, 3);
        let mut s = IpState::default();
        let t = Instant::now();
        for _ in 0..3 {
            assert!(s.admit(t, &l).is_ok());
        }
        assert_eq!(s.admit(t + Duration::from_secs(59), &l), Err(Denied::TooFrequent));
        assert!(s.admit(t + Duration::from_secs(61), &l).is_ok());
    }

    #[test]
    fn permit_drop_frees_the_slot_and_ips_are_independent() {
        let g = WsGuard::new(limits(1, 100), vec![], vec![]);
        let a: IpAddr = "203.0.113.1".parse().unwrap();
        let b: IpAddr = "203.0.113.2".parse().unwrap();
        let p = g.acquire(a).unwrap();
        assert_eq!(g.acquire(a).err(), Some(Denied::TooManyConnections));
        assert!(g.acquire(b).is_ok());
        drop(p);
        assert!(g.acquire(a).is_ok());
    }

    #[test]
    fn ipv6_counts_per_slash_64_and_mapped_v4_as_v4() {
        let g = WsGuard::new(limits(1, 100), vec![], vec![]);
        let a: IpAddr = "2001:db8:1:2::1".parse().unwrap();
        let same64: IpAddr = "2001:db8:1:2:aaaa:bbbb:cccc:dddd".parse().unwrap();
        let other64: IpAddr = "2001:db8:1:3::1".parse().unwrap();
        let _p = g.acquire(a).unwrap();
        assert!(g.acquire(same64).is_err());
        assert!(g.acquire(other64).is_ok());
        let v4: IpAddr = "203.0.113.5".parse().unwrap();
        let mapped: IpAddr = "::ffff:203.0.113.5".parse().unwrap();
        let _q = g.acquire(v4).unwrap();
        assert!(g.acquire(mapped).is_err());
    }

    #[test]
    fn rejections_do_not_create_records_and_table_is_capped() {
        let g = WsGuard::new(limits(0, 100), vec![], vec![]);
        for i in 0..10u8 {
            assert!(g.acquire(IpAddr::from([203, 0, 113, i])).is_err());
        }
        assert_eq!(g.tracked(), 0);

        let g = WsGuard::new(limits(1, 100), vec![], vec![]);
        let t = Instant::now();
        let mut permits = Vec::new();
        for i in 0..MAX_IPS as u32 {
            permits.push(g.acquire_at(IpAddr::from(i.to_be_bytes()), t).unwrap());
        }
        assert_eq!(g.tracked(), MAX_IPS);
        // 上限に達したら、新しい IP は記録を作らず断る。既存の IP の制限は効いたまま
        let new_ip = IpAddr::from([203, 0, 113, 1]);
        assert!(g.acquire_at(new_ip, t).is_err());
        assert_eq!(g.tracked(), MAX_IPS);
        // つながりが切れて窓も過ぎれば、片付いて新しい IP を受けられる
        permits.clear();
        assert!(g.acquire_at(new_ip, t + Duration::from_secs(61)).is_ok());
    }

    #[test]
    fn origin_rules() {
        let allowed = vec!["https://other.example/".to_string()];
        let ok = |o: Option<&str>, h: Option<&str>| origin_allowed(o, h, &allowed);
        // Origin 無し (非ブラウザ) は通す
        assert!(ok(None, Some("eq.example.jp")));
        // 同じ Host (ポート付きも、大文字小文字の違いも)
        assert!(ok(Some("https://eq.example.jp"), Some("eq.example.jp")));
        assert!(ok(Some("http://127.0.0.1:8080"), Some("127.0.0.1:8080")));
        assert!(ok(Some("https://EQ.example.jp"), Some("eq.example.jp")));
        // 許可リスト
        assert!(ok(Some("https://other.example"), Some("eq.example.jp")));
        // 他サイト・null・Host 無し・ポート違い
        assert!(!ok(Some("https://unrelated.example"), Some("eq.example.jp")));
        assert!(!ok(Some("null"), Some("eq.example.jp")));
        assert!(!ok(Some("https://eq.example.jp"), None));
        assert!(!ok(Some("http://127.0.0.1:9999"), Some("127.0.0.1:8080")));
    }
}
