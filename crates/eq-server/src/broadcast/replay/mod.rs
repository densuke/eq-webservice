//! 記録から音入りの動画を描き直す (`eq-server replay-video`。docs/replay-video.md の 4 章)。
//! 範囲の記録から最大震度の地震の報だけを選び、仮の時計で native の描画を回して 1 本の mp4 にする。
//! 音は mixer で作り、先に AAC にしてから映像と合わせる。e2 の詰まりを見て、詰まっていれば待つ (broadcast/psi.rs)。

mod args;
mod encode;
mod plan;
pub(crate) mod sound;
mod source;
#[cfg(test)]
mod testkit;
mod voice;
mod worker;

use std::path::PathBuf;

use anyhow::Context;

use super::native::{self, Stepper};
use super::BroadcastConfig;
use args::{Options, USAGE};
pub use worker::run as run_worker;

pub async fn run(args: &[String]) -> anyhow::Result<()> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    let o = args::parse(args)?;
    let audio = audio_path(&o.out);
    let result = make(&o, &audio).await;
    let _ = std::fs::remove_file(&audio);
    if result.is_err() {
        // 作りかけは残さない
        let _ = std::fs::remove_file(&o.out);
    }
    result
}

fn audio_path(out: &std::path::Path) -> PathBuf {
    let mut p = out.as_os_str().to_owned();
    p.push(".audio.m4a");
    p.into()
}

async fn make(o: &Options, audio: &std::path::Path) -> anyhow::Result<()> {
    let all = source::load(o).await?;
    let places = if o.quakes.is_empty() {
        plan::pick_target(&all).into_iter().collect()
    } else {
        o.quakes.clone()
    };
    let series = plan::Series::of(&all, &places).context("範囲に、動画にできる地震の報がありません")?;
    let plan = plan::build(&series, o.fps).context("筋書きを作れません")?;
    tracing::info!(
        quakes = series.members.len(),
        events = series.events.len(),
        frames = plan.frames.len(),
        seconds = plan::duration_ms(plan.frames.len(), o.fps) / 1000,
        "replay-video: 筋書きができました"
    );
    for (at, from, to) in plan::jumps(&plan.frames, o.fps) {
        tracing::info!(
            "replay-video: 早送り {:>6.1}秒 ({} -> {})",
            at as f64 / 1000.0,
            crate::quake::jst::format(from as i64),
            crate::quake::jst::format(to as i64)
        );
    }
    for s in &plan.sounds {
        tracing::info!(
            "replay-video: 音 {:>6.1}秒 ({}) {:?} {}",
            s.video_ms as f64 / 1000.0,
            crate::quake::jst::format(s.now_ms as i64),
            s.level,
            s.why
        );
    }
    let chapters = plan::chapters(&plan.frames, &series.members, o.fps);
    for c in &chapters {
        tracing::info!(
            "replay-video: 地震 {:>6.1}秒 {} {} 震度{}",
            c.video_ms as f64 / 1000.0,
            c.origin_ms.map_or_else(String::new, crate::quake::jst::format),
            c.name,
            crate::quake::Scale(c.max_scale).label()
        );
    }
    if let Some(path) = &o.chapters {
        std::fs::write(path, serde_json::to_vec_pretty(&chapters)?)
            .with_context(|| format!("writing {}", path.display()))?;
    }
    let mut throttle = crate::broadcast::psi::Throttle::default();
    let voices = fetch_voices(o, &series.events, &plan).await;
    encode::encode_audio(o, &plan, &voices, audio, &mut throttle).await?;
    let mut stepper = Stepper::new(native::load_renderer(&renderer_config(o))?);
    encode::encode_video(o, &series.events, &plan, &mut stepper, audio, &mut throttle).await?;
    tracing::info!(out = %o.out.display(), "replay-video: できました");
    Ok(())
}

/// --voice があれば、読む報の声をサーバから取る。取れなければ (通信できないなど) 警告だけで、声なしで作る
async fn fetch_voices(o: &Options, events: &[crate::quake::Event], plan: &plan::Plan) -> Vec<voice::Clip> {
    let Some(base) = &o.voice else {
        return Vec::new();
    };
    let api = match voice::Http::new(base) {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!("replay-video: 読み上げの接続を作れません。声なしで作ります: {e:#}");
            return Vec::new();
        }
    };
    let slots = voice::schedule(events, &plan.frames, o.fps);
    let clips = voice::collect(&api, events, &slots, voice::PAUSE).await;
    for c in &clips {
        tracing::info!(
            "replay-video: 声 {:>6.1}秒 ({:.1}秒分)",
            c.at_ms as f64 / 1000.0,
            c.pcm.len() as f64 / f64::from(crate::broadcast::mixer::RATE)
        );
    }
    tracing::info!("replay-video: 読む報 {} 件のうち、声 {} 本", slots.len(), clips.len());
    clips
}

/// 描画に要る設定 (地図・フォント)。ほかは配信の既定のまま
fn renderer_config(o: &Options) -> BroadcastConfig {
    BroadcastConfig {
        map_dir: o.map_dir.clone(),
        font: o.font.clone(),
        zoom: o.zoom,
        ..BroadcastConfig::default()
    }
}
