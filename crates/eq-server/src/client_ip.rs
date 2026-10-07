//! 接続元 IP の決め方 (1 か所)。
//!
//! 直接の相手 (peer) が信頼するプロキシのときだけ `X-Forwarded-For` の最も右の値
//! (= 信頼プロキシが見た接続元) を使い、それ以外は peer。任意の `X-Forwarded-For` は信用しない。

use std::net::IpAddr;

use anyhow::Context;
use axum::http::HeaderMap;

/// 設定の trusted_proxies (IP アドレスの文字列) を解釈する。書き損じは起動時に止める
pub fn parse_trusted(list: &[String]) -> anyhow::Result<Vec<IpAddr>> {
    list.iter()
        .map(|s| {
            s.trim()
                .parse::<IpAddr>()
                .map(|ip| ip.to_canonical())
                .with_context(|| format!("[server] trusted_proxies: IP アドレスではありません: {s:?}"))
        })
        .collect()
}

/// 接続元 IP。`trusted` は `parse_trusted` の結果
pub fn client_ip(peer: IpAddr, headers: &HeaderMap, trusted: &[IpAddr]) -> IpAddr {
    let peer = peer.to_canonical();
    if !trusted.contains(&peer) {
        return peer;
    }
    headers
        .get_all("x-forwarded-for")
        .iter()
        .next_back()
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit(',').next())
        .and_then(|last| last.trim().parse::<IpAddr>().ok())
        .map_or(peer, |ip| ip.to_canonical())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }
    fn xff(values: &[&str]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for v in values {
            h.append("x-forwarded-for", v.parse().unwrap());
        }
        h
    }
    fn trusted() -> Vec<IpAddr> {
        parse_trusted(&["127.0.0.1".into(), "::1".into()]).unwrap()
    }

    #[test]
    fn untrusted_peer_ignores_forwarded_for() {
        assert_eq!(
            client_ip(ip("198.51.100.7"), &xff(&["203.0.113.9"]), &trusted()),
            ip("198.51.100.7")
        );
    }

    #[test]
    fn trusted_peer_uses_rightmost_value() {
        // 左側は利用者が自由に書ける。信頼プロキシが足した最も右だけを使う
        let h = xff(&["1.2.3.4, 5.6.7.8", "203.0.113.9"]);
        assert_eq!(client_ip(ip("127.0.0.1"), &h, &trusted()), ip("203.0.113.9"));
        let h = xff(&["10.0.0.1, 203.0.113.9"]);
        assert_eq!(client_ip(ip("::1"), &h, &trusted()), ip("203.0.113.9"));
    }

    #[test]
    fn trusted_peer_without_or_with_bad_header_falls_back_to_peer() {
        assert_eq!(
            client_ip(ip("127.0.0.1"), &HeaderMap::new(), &trusted()),
            ip("127.0.0.1")
        );
        assert_eq!(
            client_ip(ip("127.0.0.1"), &xff(&["garbage"]), &trusted()),
            ip("127.0.0.1")
        );
    }

    #[test]
    fn ipv4_mapped_peer_is_canonicalized() {
        assert_eq!(
            client_ip(ip("::ffff:127.0.0.1"), &xff(&["203.0.113.9"]), &trusted()),
            ip("203.0.113.9")
        );
    }

    #[test]
    fn parse_trusted_rejects_garbage() {
        assert!(parse_trusted(&["localhost".into()]).is_err());
    }
}
