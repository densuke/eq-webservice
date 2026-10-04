//! ループバックのリダイレクトの受け取り。ブラウザが `http://127.0.0.1:<port>/?code=...&state=...` に来たときの 1 行目を読む。

use std::time::Duration;

use anyhow::Context;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Debug, PartialEq, Eq)]
pub enum Redirect {
    /// 認可コード
    Code(String),
    /// 利用者が断った・state が違う・code が無い
    Failed(String),
    /// リダイレクトではない (favicon の取得など)。無視して待ち続ける
    Other,
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = b.get(i + 1..i + 3).filter(|h| h.iter().all(u8::is_ascii_hexdigit));
        match (b[i], hex) {
            (b'+', _) => out.push(b' '),
            (b'%', Some(h)) => {
                out.push(u8::from_str_radix(std::str::from_utf8(h).unwrap_or("3f"), 16).unwrap_or(b'?'));
                i += 2;
            }
            (c, _) => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// リクエストの 1 行目 ("GET /?code=..&state=.. HTTP/1.1") を読む
pub fn parse_request_line(line: &str, expected_state: &str) -> Redirect {
    let mut it = line.split_whitespace();
    let (Some("GET"), Some(target)) = (it.next(), it.next()) else {
        return Redirect::Other;
    };
    let Some(query) = target.strip_prefix("/?") else {
        return Redirect::Other;
    };
    let get = |name: &str| {
        query
            .split('&')
            .filter_map(|kv| kv.split_once('='))
            .find(|(k, _)| *k == name)
            .map(|(_, v)| percent_decode(v))
    };
    // state が合わないものは、コードがあっても受け取らない (別のところから来たリダイレクト)
    if get("state").as_deref() != Some(expected_state) {
        return Redirect::Failed("state が一致しません (別のリダイレクトかもしれません)".into());
    }
    if let Some(err) = get("error") {
        return Redirect::Failed(format!("同意が得られませんでした: {err}"));
    }
    match get("code") {
        Some(code) if !code.is_empty() => Redirect::Code(code),
        _ => Redirect::Failed("リダイレクトに code がありません".into()),
    }
}

const PAGE: &str = "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\n\r\n\
認証を受け取りました。このタブは閉じて、ターミナルに戻ってください。";
const NOT_FOUND: &str = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

/// 認可コードが届くまで待つ (最大 timeout)。favicon などは無視する
pub async fn wait_for_code(listener: &TcpListener, state: &str, timeout: Duration) -> anyhow::Result<String> {
    let wait = async {
        loop {
            let (mut sock, _) = listener.accept().await.context("accept")?;
            let mut buf = vec![0u8; 8192];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]);
            let parsed = parse_request_line(head.lines().next().unwrap_or(""), state);
            let reply = if parsed == Redirect::Other { NOT_FOUND } else { PAGE };
            let _ = sock.write_all(reply.as_bytes()).await;
            match parsed {
                Redirect::Code(c) => return Ok(c),
                Redirect::Failed(why) => anyhow::bail!(why),
                Redirect::Other => {}
            }
        }
    };
    tokio::time::timeout(timeout, wait)
        .await
        .context("同意の画面での操作を待ちきれませんでした")?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_redirect_with_the_right_state_gives_the_code() {
        assert_eq!(
            parse_request_line("GET /?state=abc&code=4%2F0AbC-d HTTP/1.1", "abc"),
            Redirect::Code("4/0AbC-d".into())
        );
    }

    #[test]
    fn a_wrong_or_missing_state_is_rejected() {
        assert!(matches!(
            parse_request_line("GET /?state=x&code=c HTTP/1.1", "abc"),
            Redirect::Failed(_)
        ));
        assert!(matches!(
            parse_request_line("GET /?code=c HTTP/1.1", "abc"),
            Redirect::Failed(_)
        ));
    }

    #[test]
    fn an_error_parameter_is_a_failure_even_with_the_right_state() {
        match parse_request_line("GET /?error=access_denied&state=abc HTTP/1.1", "abc") {
            Redirect::Failed(why) => assert!(why.contains("access_denied")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_redirect_without_a_code_fails() {
        assert!(matches!(
            parse_request_line("GET /?state=abc HTTP/1.1", "abc"),
            Redirect::Failed(_)
        ));
        assert!(matches!(
            parse_request_line("GET /?state=abc&code= HTTP/1.1", "abc"),
            Redirect::Failed(_)
        ));
    }

    #[test]
    fn other_requests_are_ignored() {
        assert_eq!(parse_request_line("GET /favicon.ico HTTP/1.1", "abc"), Redirect::Other);
        assert_eq!(
            parse_request_line("POST /?code=c&state=abc HTTP/1.1", "abc"),
            Redirect::Other
        );
        assert_eq!(parse_request_line("", "abc"), Redirect::Other);
    }

    #[tokio::test]
    async fn waiting_returns_the_code_after_ignoring_a_favicon_request() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = tokio::spawn(async move {
            for path in ["/favicon.ico", "/?state=s1&code=THE-CODE"] {
                let mut c = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
                c.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                    .await
                    .unwrap();
                let mut sink = Vec::new();
                let _ = c.read_to_end(&mut sink).await;
            }
        });
        let code = wait_for_code(&listener, "s1", Duration::from_secs(5)).await.unwrap();
        assert_eq!(code, "THE-CODE");
        client.await.unwrap();
    }

    #[tokio::test]
    async fn waiting_times_out_when_nobody_comes() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        assert!(wait_for_code(&listener, "s", Duration::from_millis(50)).await.is_err());
    }
}
