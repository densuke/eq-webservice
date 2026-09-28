mod config;
mod http;
mod hub;
mod plugins;
mod source;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::hub::Hub;

const USAGE: &str = "usage: eq-server [--config <path>]\n\n\
設定ファイルを省略した場合は ./config.toml を探し、無ければ既定値 (P2P地震情報に接続、127.0.0.1:8080 で待受) で起動します。";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let cfg = load_config()?;
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

fn load_config() -> anyhow::Result<Config> {
    let mut args = std::env::args().skip(1);
    let mut path: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "-c" | "--config" => path = Some(args.next().context("--config needs a path")?.into()),
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => anyhow::bail!("unknown argument {other:?}\n{USAGE}"),
        }
    }
    match path {
        Some(p) => Config::load(&p),
        None if std::path::Path::new("config.toml").exists() => Config::load("config.toml".as_ref()),
        None => {
            tracing::info!("no config file, using defaults");
            Config::parse("")
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
