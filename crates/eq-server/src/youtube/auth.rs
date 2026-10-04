//! `eq-server youtube-auth --client <client.json> --token <token.json>`: 一度だけ人がやる同意の手続き。
//! OAuth 2.0 の installed app (ループバックのリダイレクト・PKCE)。範囲は動画を上げる権限だけ。

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use tokio::net::TcpListener;

use super::api::Google;
use super::loopback;
use super::pkce::{consent_url, Pkce};
use super::token::{self, AuthError, TokenFile, SCOPE};

pub const USAGE: &str = "\
usage: eq-server youtube-auth --client <client.json> --token <token.json>
  Google Cloud の「デスクトップアプリ」の OAuth クライアント (JSON) を使って、YouTube に動画を上げる許可を 1 度だけ得る。
  同意の URL を表示する (macOS ではブラウザも開く)。同意すると、リフレッシュトークンを token.json に書く (モード 0600)。
  許可の範囲は https://www.googleapis.com/auth/youtube.upload だけ (動画を上げるだけで、読み出し・削除はできない)。";

/// 同意の画面での操作を待つ時間
const WAIT: Duration = Duration::from_secs(300);

struct Args {
    client: PathBuf,
    token: PathBuf,
}

fn parse_args(args: &[String]) -> anyhow::Result<Args> {
    let (mut client, mut token) = (None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--client" => client = Some(it.next().context("--client needs a value")?.into()),
            "--token" => token = Some(it.next().context("--token needs a value")?.into()),
            other => anyhow::bail!("unknown argument {other:?}\n\n{USAGE}"),
        }
    }
    Ok(Args {
        client: client.with_context(|| format!("--client が要ります\n\n{USAGE}"))?,
        token: token.with_context(|| format!("--token が要ります\n\n{USAGE}"))?,
    })
}

pub async fn run(args: &[String]) -> anyhow::Result<()> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    let a = parse_args(args)?;
    let client = token::load_client(&a.client)?;
    let pkce = Pkce::generate()?;
    // ポートは OS に選ばせる。127.0.0.1 だけで待つ (外からは届かない)
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("ループバックで待てません")?;
    let redirect_uri = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
    let url = consent_url(&client, &redirect_uri, SCOPE, &pkce);
    println!("ブラウザで次の URL を開いて、YouTube への動画の追加を許可してください (範囲: {SCOPE})。\n\n{url}\n");
    open_in_browser(&url);
    println!(
        "許可すると、このコマンドが自動で続きを進めます (最大 {} 分待ちます)。",
        WAIT.as_secs() / 60
    );

    let code = loopback::wait_for_code(&listener, &pkce.state, WAIT).await?;
    let fresh = Google::new()?
        .exchange(&client, &code, &pkce.verifier, &redirect_uri, super::now_ms())
        .await
        .map_err(|e| match e {
            AuthError::InvalidGrant => {
                anyhow::anyhow!("認可コードを受け付けてもらえませんでした (invalid_grant)。もう一度やり直してください")
            }
            AuthError::Other(why) => anyhow::anyhow!(why),
        })?;
    let refresh_token = fresh.refresh_token.context(
        "リフレッシュトークンが返りませんでした。https://myaccount.google.com/permissions でこのアプリの許可を取り消してから、もう一度やり直してください",
    )?;
    token::save_token(
        &a.token,
        &TokenFile {
            refresh_token,
            access_token: Some(fresh.access_token),
            expires_at_ms: Some(fresh.expires_at_ms),
        },
    )?;
    println!("トークンを {} に書きました (モード 0600)。", a.token.display());
    Ok(())
}

/// macOS では、同意の URL を開く。失敗しても、表示した URL を手で開けばよい
fn open_in_browser(url: &str) {
    if cfg!(target_os = "macos") {
        let _ = std::process::Command::new("open").arg(url).status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn both_paths_are_required() {
        let a = parse_args(&args(&["--client", "c.json", "--token", "t.json"])).unwrap();
        assert_eq!((a.client, a.token), ("c.json".into(), "t.json".into()));
        assert!(parse_args(&args(&["--client", "c.json"])).is_err());
        assert!(parse_args(&args(&["--token", "t.json"])).is_err());
        assert!(parse_args(&args(&["--client"])).is_err());
        assert!(parse_args(&args(&["--nope"])).is_err());
    }
}
