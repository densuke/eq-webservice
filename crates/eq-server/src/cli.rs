//! 起動オプション。設定ファイルより優先される。

use std::path::PathBuf;

use anyhow::Context;

pub const USAGE: &str = "\
usage: eq-server [options]
       eq-server convert <samples/scenarios> <web/public/demo>   (デモモード用の JSON を作る)
       eq-server replay-video --from <ms> --to <ms> --out <x.mp4> (--events <jsonl> | --archive <URL>)
                                                          (記録から音入りの動画を描き直す。詳しくは replay-video --help)
       eq-server bgm-send <MP3 のディレクトリ> <http://127.0.0.1:8010/bgm.mp3>
                                                          (平時の BGM を Icecast へ送る。パスワードは ICECAST_SOURCE_PASSWORD)

options:
  -c, --config <path>      設定ファイル (省略時は ./config.toml、無ければ既定値)
  -l, --listen <addr:port> 待ち受けアドレス (例: 127.0.0.1:8080, 0.0.0.0:3000)
  -p, --port <port>        待ち受けポートだけを変更 (アドレスは設定ファイルのまま)
      --static-dir <path>  ページのファイル (web/dist) の場所
  -V, --version            バージョンを表示
  -h, --help               このヘルプを表示

environment:
  EQ_LISTEN, EQ_PORT       --listen / --port と同じ (起動オプションが優先)
  RUST_LOG                 ログレベル (例: info, eq_server=debug)";

#[derive(Debug, Default, PartialEq)]
pub struct Cli {
    pub config: Option<PathBuf>,
    pub listen: Option<String>,
    pub port: Option<u16>,
    pub static_dir: Option<PathBuf>,
    pub help: bool,
    pub version: bool,
}

impl Cli {
    pub fn parse(args: impl IntoIterator<Item = String>) -> anyhow::Result<Cli> {
        let mut cli = Cli::default();
        let mut args = args.into_iter();
        while let Some(key) = args.next() {
            let mut value = |name: &str| args.next().with_context(|| format!("{name} needs a value"));
            match key.as_str() {
                "-c" | "--config" => cli.config = Some(value("--config")?.into()),
                "-l" | "--listen" => cli.listen = Some(value("--listen")?),
                "-p" | "--port" => cli.port = Some(parse_port(&value("--port")?)?),
                "--static-dir" => cli.static_dir = Some(value("--static-dir")?.into()),
                "-h" | "--help" => cli.help = true,
                "-V" | "--version" => cli.version = true,
                other => anyhow::bail!("unknown argument {other:?}\n\n{USAGE}"),
            }
        }
        Ok(cli)
    }

    /// 起動オプションが無ければ環境変数で補う
    pub fn with_env(mut self, get: impl Fn(&str) -> Option<String>) -> anyhow::Result<Cli> {
        if self.listen.is_none() {
            self.listen = get("EQ_LISTEN").filter(|v| !v.is_empty());
        }
        if self.port.is_none() {
            if let Some(p) = get("EQ_PORT").filter(|v| !v.is_empty()) {
                self.port = Some(parse_port(&p).context("EQ_PORT")?);
            }
        }
        Ok(self)
    }

    /// 設定ファイルの listen に --listen / --port を反映した値
    pub fn apply_listen(&self, configured: &str) -> String {
        let base = self.listen.as_deref().unwrap_or(configured);
        match self.port {
            Some(port) => {
                // "[::1]:8080" のような IPv6 表記も考慮して最後の ':' で分ける
                let host = base.rsplit_once(':').map(|(h, _)| h).unwrap_or(base);
                format!("{host}:{port}")
            }
            None => base.to_string(),
        }
    }
}

fn parse_port(s: &str) -> anyhow::Result<u16> {
    s.parse().with_context(|| format!("invalid port {s:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> anyhow::Result<Cli> {
        Cli::parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_options() {
        let cli = parse(&["-c", "a.toml", "--port", "3000", "--static-dir", "/srv/eq"]).unwrap();
        assert_eq!(cli.config, Some("a.toml".into()));
        assert_eq!(cli.port, Some(3000));
        assert_eq!(cli.static_dir, Some("/srv/eq".into()));
        assert!(parse(&["--port", "abc"]).is_err());
        assert!(parse(&["--nope"]).is_err());
        assert!(parse(&["--port"]).is_err());
    }

    #[test]
    fn port_replaces_only_the_port() {
        let cli = parse(&["-p", "9000"]).unwrap();
        assert_eq!(cli.apply_listen("127.0.0.1:8080"), "127.0.0.1:9000");
        assert_eq!(cli.apply_listen("[::1]:8080"), "[::1]:9000");
        let cli = parse(&["--listen", "0.0.0.0:80", "-p", "81"]).unwrap();
        assert_eq!(cli.apply_listen("127.0.0.1:8080"), "0.0.0.0:81");
        assert_eq!(parse(&[]).unwrap().apply_listen("127.0.0.1:8080"), "127.0.0.1:8080");
    }

    #[test]
    fn env_is_used_only_without_options() {
        let env = |k: &str| match k {
            "EQ_PORT" => Some("7000".to_string()),
            "EQ_LISTEN" => Some("0.0.0.0:1".to_string()),
            _ => None,
        };
        let cli = parse(&[]).unwrap().with_env(env).unwrap();
        assert_eq!(cli.apply_listen("127.0.0.1:8080"), "0.0.0.0:7000");
        let cli = parse(&["-p", "5000"]).unwrap().with_env(env).unwrap();
        assert_eq!(cli.port, Some(5000));
        assert!(parse(&[]).unwrap().with_env(|_| Some("x".into())).is_err());
    }
}
