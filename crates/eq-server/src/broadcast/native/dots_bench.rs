//! 能登 (2024-01-01) の記録で、観測点の点あり・なしの 1 コマ (render) の重さを測る (#212)。
//! `EQ_NATIVE_FONT=... EQ_DOTS_OUT=/tmp/dots cargo test --release -p eq-server station_dots_cost -- --ignored --nocapture`

use super::draw::Scene;
use super::spread::Spread;
use super::zoom_tests::{config_sub, input};
use super::*;
use crate::quake::EventBody;

#[test]
#[ignore]
fn station_dots_cost() {
    const RENDERS: usize = 100;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text = std::fs::read_to_string(root.join("samples/scenarios/noto2024.jsonl")).unwrap();
    let events = crate::source::replay::load(&text).unwrap();
    // 観測点の最も多い地震情報の受信時刻
    let (at, n) = events
        .iter()
        .filter_map(|e| match &e.body {
            EventBody::Quake(q) => Some((e.received_at_ms, q.points.iter().filter(|p| !p.is_area).count())),
            _ => None,
        })
        .max_by_key(|&(_, n)| n)
        .unwrap();
    let now = at + 10_000;
    let mut bare = events.clone();
    for e in &mut bare {
        if let EventBody::Quake(q) = &mut e.body {
            q.points.clear();
        }
    }
    let icons = Icons::new();
    for zoom in [false, true] {
        let mut cfg = config_sub(zoom, false);
        cfg.font = std::env::var("EQ_NATIVE_FONT").unwrap_or(cfg.font);
        let mut st = Stepper::new(load_renderer(&cfg).unwrap());
        for k in 0..10 {
            st.step(&input(&events, now + k * 200, &icons)); // 温める (寄りの範囲も入る)
        }
        // spread: 揺れの広がりの再現の最中 (再現の発生から最大 20 秒。点を S 波の到達で絞る)
        for (label, ev, spreading) in [
            ("dots", &events, false),
            ("none", &bare, false),
            ("spread", &events, true),
        ] {
            let groups = model::group_quakes(ev);
            let quake = groups.iter().find(|g| g.updated_ms >= at).or(groups.first());
            let spread = quake
                .and_then(|q| q.hypocenter.as_ref())
                .filter(|_| spreading)
                .map(|h| Spread {
                    lat: h.latitude.unwrap(),
                    lon: h.longitude.unwrap(),
                    depth_km: h.depth_km.map_or(10.0, f64::from),
                    start_ms: now.saturating_sub(20_000),
                    end_ms: now + 60_000,
                });
            let scene = Scene {
                quake,
                eew: None,
                history: &groups,
                warnings: None,
                tsunami: &[],
                weather: None,
                icons: &icons,
                flip_s: 0,
                now_ms: now,
                connected: true,
                bgm_title: "",
                label: "",
                test: false,
                hindsight: None,
                fast_forward: false,
                status: None,
                viewers: None,
                notices: None,
                spread,
            };
            let mut ms: Vec<f64> = (0..RENDERS)
                .map(|_| {
                    let t = std::time::Instant::now();
                    std::hint::black_box(st.renderer_mut().render(&scene));
                    t.elapsed().as_secs_f64() * 1000.0
                })
                .collect();
            ms.sort_by(f64::total_cmp);
            println!(
                "points={n} zoom={zoom} {label}: render med {:.2} p95 {:.2} ms",
                ms[RENDERS / 2],
                ms[RENDERS * 95 / 100]
            );
            if let Ok(out) = std::env::var("EQ_DOTS_OUT") {
                let pm = st.renderer_mut().render(&scene);
                pm.save_png(std::path::Path::new(&out).join(format!("noto_zoom{zoom}_{label}.png")))
                    .unwrap();
            }
        }
    }
}
