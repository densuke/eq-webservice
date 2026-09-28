mod cli;
mod config;
mod http;
mod hub;
mod plugins;
mod source;
mod telop;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use tracing_subscriber::EnvFilter;

use crate::cli::{Cli, USAGE};
use crate::config::Config;
use crate::hub::Hub;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let cli = Cli::parse(std::env::args().skip(1))?.with_env(|k| std::env::var(k).ok())?;
    if cli.help {
        println!("{USAGE}");
        return Ok(());
    }
    if cli.version {
        println!("eq-server {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let cfg = load_config(&cli)?;
    tracing::info!(static_dir = %cfg.server.static_dir.display(), "eq-server {}", env!("CARGO_PKG_VERSION"));
    let hub = Hub::new(cfg.server.recent_capacity);

    // プラグインは取得元より先に購読させる (起動直後のイベントを取りこぼさないため)
    let mut routes = Vec::new();
    let mut seeders = Vec::new();
    for table in &cfg.sinks {
        let Some(loaded) = plugins::build(table)? else { continue };
        tracing::info!(sink = %loaded.name, "sink enabled");
        if let Some(r) = loaded.sink.clone().routes() {
            routes.push(r);
        }
        seeders.push((loaded.name.clone(), loaded.sink.clone(), loaded.filter.clone()));
        plugins::spawn(loaded, &hub);
    }

    let eew_enabled = matches!(&cfg.source, config::SourceConfig::P2pquake { eew_url, .. } if !eew_url.is_empty());
    routes.push(telop::router(telop::messages(&cfg.telop.messages, eew_enabled)));

    let listener = tokio::net::TcpListener::bind(&cfg.server.listen)
        .await
        .with_context(|| format!("binding {}", cfg.server.listen))?;
    tracing::info!("listening on http://{}", listener.local_addr()?);
    let app = http::router(hub.clone(), &cfg.server.static_dir, routes);

    let source_hub = hub.clone();
    tokio::spawn(source::run(cfg.source, source_hub, move |seeded| {
        for (name, sink, filter) in seeders {
            let events: Vec<Arc<eq_core::Event>> = seeded.iter().filter(|e| filter.accepts(e)).cloned().collect();
            tokio::spawn(async move {
                if let Err(e) = sink.seed(&events).await {
                    tracing::warn!(sink = %name, "seed failed: {e:#}");
                }
            });
        }
    }));

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn load_config(cli: &Cli) -> anyhow::Result<Config> {
    let mut cfg = match &cli.config {
        Some(p) => Config::load(p)?,
        None if Path::new("config.toml").exists() => Config::load("config.toml".as_ref())?,
        None => {
            tracing::info!("no config file, using defaults");
            Config::parse("")?
        }
    };
    cfg.server.listen = cli.apply_listen(&cfg.server.listen);
    if let Some(dir) = &cli.static_dir {
        cfg.server.static_dir = dir.clone();
    }
    cfg.server.static_dir = resolve_static_dir(&cfg.server.static_dir);
    Ok(cfg)
}

/// 相対パスが作業ディレクトリに無ければ、実行ファイルの隣を探す
/// (リリース版を展開した場所以外から起動した場合でもページを配信できるように)。
fn resolve_static_dir(dir: &Path) -> PathBuf {
    if dir.as_os_str().is_empty() || dir.is_absolute() || dir.is_dir() {
        return dir.to_path_buf();
    }
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join(dir)))
        .filter(|p| p.is_dir());
    match beside_exe {
        Some(p) => p,
        None => {
            tracing::warn!(
                "static_dir {} not found; the map page will not be served",
                dir.display()
            );
            dir.to_path_buf()
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = term => {},
    }
    tracing::info!("shutting down");
}
