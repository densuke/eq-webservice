mod archive;
mod banner;
mod bgm;
mod bgm_send;
mod broadcast;
mod city_weather;
mod cli;
mod client_ip;
mod config;
mod demo;
mod http;
mod hub;
mod layout;
mod net;
mod plugins;
mod quake;
mod source;
mod telop;
mod tts;
mod weather;
mod ws_guard;
mod youtube;
mod youtube_viewers;

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

    let args: Vec<String> = std::env::args().skip(1).collect();
    // eq-server convert <場面のディレクトリ> <出力先>: デモモード用の JSON を作る
    if args.first().map(String::as_str) == Some("convert") {
        let [_, src, out] = args.as_slice() else {
            anyhow::bail!("usage: eq-server convert <samples/scenarios> <web/public/demo>");
        };
        return demo::convert_dir(src.as_ref(), out.as_ref());
    }
    // eq-server bgm-send <MP3 のディレクトリ> <Icecast の URL>: 平時の BGM を Icecast へ送り続ける
    if args.first().map(String::as_str) == Some("bgm-send") {
        return bgm_send::run(&args[1..]);
    }
    // eq-server broadcast <broadcast.toml>: 地図のページを ffmpeg で配信し続ける
    if args.first().map(String::as_str) == Some("broadcast") {
        return broadcast::run(&args[1..]).await;
    }
    // eq-server replay-video --from <ms> --to <ms> --out x.mp4 ...: 記録から音入りの動画を描き直す
    if args.first().map(String::as_str) == Some("replay-video") {
        return broadcast::replay_video(&args[1..]).await;
    }
    // eq-server replay-worker [replay.toml]: 記録から動画にする地震を見つけて、キューに積み、空き時間に作る
    if args.first().map(String::as_str) == Some("replay-worker") {
        return broadcast::replay_worker(&args[1..]).await;
    }
    // eq-server youtube-auth --client <client.json> --token <token.json>: YouTube に上げる許可を一度だけ得る
    if args.first().map(String::as_str) == Some("youtube-auth") {
        return youtube::auth::run(&args[1..]).await;
    }
    let cli = Cli::parse(args)?.with_env(|k| std::env::var(k).ok())?;
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
        if let Some(r) = loaded.sink.routes() {
            routes.push(r);
        }
        seeders.push((loaded.name.clone(), loaded.sink.clone(), loaded.filter.clone()));
        plugins::spawn(loaded, &hub);
    }

    let eew_enabled = matches!(&cfg.source, config::SourceConfig::P2pquake { eew_url, .. } if !eew_url.is_empty());
    routes.push(telop::router(telop::messages(&cfg.telop.messages, eew_enabled)));
    let warnings = weather::Shared::default();
    if cfg.weather.enabled {
        weather::spawn(cfg.weather.clone(), warnings.clone());
    }
    routes.push(weather::router(warnings));
    let city = city_weather::Shared::default();
    if cfg.weather.enabled {
        city_weather::spawn(city.clone());
    }
    routes.push(city_weather::router(city));
    let viewers = youtube_viewers::Shared::default();
    if cfg.viewers.enabled {
        match cfg.viewers.check() {
            Ok(()) => {
                if cfg.viewers.interval().as_secs() != cfg.viewers.interval_sec {
                    tracing::warn!(
                        interval_sec = cfg.viewers.interval_sec,
                        "viewers: interval_sec は 10〜300 秒に収めます"
                    );
                }
                youtube_viewers::spawn(cfg.viewers.clone(), viewers.clone())
            }
            Err(e) => tracing::warn!("{e}"),
        }
    }
    routes.push(youtube_viewers::router(viewers));
    routes.push(http::source_router(cfg.source.kind()));
    routes.push(bgm::router(&cfg.bgm));
    routes.push(banner::router(&cfg.banner));
    routes.push(layout::router(&cfg.layout));
    // jsonl の sink があるときだけ、その記録を返す
    let archive_path = archive::jsonl_path(&cfg.sinks);
    if let Some(path) = archive_path.clone() {
        routes.push(archive::router(path));
    }

    // 音声アナウンス。キーが無ければ警告して無効 (エンドポイントは 404)
    let tts_cache = build_tts(&cfg.tts, archive_path, cfg.server.static_dir.join("demo"))?;
    let tts_token = std::env::var("EQ_TTS_TOKEN").ok().filter(|t| !t.is_empty());
    let announce_rate = cfg.server.rate_guard(cfg.tts.announce_per_min)?;
    routes.push(tts::http::router(hub.clone(), tts_cache, tts_token, announce_rate));

    let listener = tokio::net::TcpListener::bind(&cfg.server.listen)
        .await
        .with_context(|| format!("binding {}", cfg.server.listen))?;
    tracing::info!("listening on http://{}", listener.local_addr()?);
    let ws_guard = cfg.server.ws_guard()?;
    let app = http::router(hub.clone(), &cfg.server.static_dir, routes, ws_guard);

    let source_hub = hub.clone();
    tokio::spawn(source::run(cfg.source, source_hub, move |seeded| {
        for (name, sink, filter) in seeders {
            let events: Vec<Arc<crate::quake::Event>> = seeded.iter().filter(|e| filter.accepts(e)).cloned().collect();
            tokio::spawn(async move {
                if let Err(e) = sink.seed(&events).await {
                    tracing::warn!(sink = %name, "seed failed: {e:#}");
                }
            });
        }
    }));

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
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

/// `[tts]` からキャッシュを作る。無効、またはキー未設定なら None
fn build_tts(
    cfg: &tts::TtsConfig,
    archive: Option<std::path::PathBuf>,
    demo_dir: std::path::PathBuf,
) -> anyhow::Result<Option<Arc<tts::cache::Cache<tts::google::Google>>>> {
    if !cfg.enabled {
        return Ok(None);
    }
    let Some(key) = std::env::var("GOOGLE_TTS_API_KEY").ok().filter(|k| !k.is_empty()) else {
        tracing::warn!("tts enabled but GOOGLE_TTS_API_KEY is not set; tts disabled");
        return Ok(None);
    };
    let budget = tts::budget::Budget::load(cfg.cache_dir.join("usage.json"), cfg.monthly_char_limit);
    let cache = Arc::new(tts::cache::Cache::new(
        cfg.cache_dir.clone(),
        cfg.voice.clone(),
        tts::google::Google::new(key)?,
        budget,
    ));
    if cfg.prewarm {
        tts::prewarm::spawn_prewarm(cache.clone(), archive, Some(demo_dir));
    }
    tracing::info!(voice = %cfg.voice, "tts enabled");
    Ok(Some(cache))
}
