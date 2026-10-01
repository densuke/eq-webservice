//! 寄り (zoom) の描画の確認 (実際の地図データで描く。文字は描かない設定で、CI にフォントが無くてもよい)。
//! 2026-10-01 21:27 の千葉県北東部の地震 (緊急地震速報の予想) を使う。

use super::draw::Scene;
use super::*;
use crate::quake::{Eew, EewArea, EventBody, Hypocenter, PrefScale, Scale};

const T0: i64 = 1_790_000_000_000;

fn config(zoom: bool) -> BroadcastConfig {
    BroadcastConfig {
        map_dir: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../web/public")
            .display()
            .to_string(),
        font: "/nonexistent".into(),
        zoom,
        ..BroadcastConfig::default()
    }
}

fn area(name: &str, from: Scale, to: Scale) -> EewArea {
    EewArea {
        pref: String::new(),
        name: name.into(),
        scale_from: from,
        scale_to: Some(to),
        arrival_time: None,
        arrived: false,
    }
}

/// 千葉県北東部の沖 (35.7N 140.8E) の緊急地震速報。第 5 報で千葉県北東部と茨城県南部が震度 4
fn chiba_events() -> Vec<Event> {
    let pref = |p: &str| PrefScale {
        pref: p.into(),
        scale: Scale::S4,
    };
    vec![Event {
        id: "a-5".into(),
        source: "test".into(),
        received_at_ms: (T0 + 58_000) as u64,
        body: EventBody::Eew(Eew {
            event_id: "a".into(),
            serial: "5".into(),
            cancelled: false,
            test: false,
            warning: false,
            issued_at: "2026/10/01 21:27:48".into(),
            origin_time: Some("2026/10/01 21:26:50".into()),
            origin_time_ms: Some(T0),
            hypocenter: Some(Hypocenter {
                name: "千葉県北東部".into(),
                latitude: Some(35.7),
                longitude: Some(140.8),
                depth_km: Some(40),
                magnitude: Some(4.5),
            }),
            areas: vec![
                area("千葉県北東部", Scale::S4, Scale::S4),
                area("茨城県南部", Scale::S3, Scale::S4),
            ],
            pref_max: vec![pref("千葉県"), pref("茨城県")],
            max_scale: Scale::S4,
        }),
    }]
}

fn input<'a>(events: &'a [Event], now: u64, icons: &'a Icons) -> Input<'a> {
    Input {
        events,
        now,
        rev: 1,
        warnings: None,
        weather: None,
        icons,
        bgm_title: "",
        connected: true,
        label: "",
        test: false,
        check_ms: 200,
        flip_s: 0,
        hindsight: None,
        fast_forward: false,
    }
}

/// 200ms ごとに n コマ進め、最後に描いた画面を返す
fn run(st: &mut Stepper, events: &[Event], from_ms: u64, n: u64) -> Vec<u8> {
    let icons = Icons::new();
    let mut last = Vec::new();
    for k in 0..n {
        if let Some(o) = st.step(&input(events, from_ms + k * 200, &icons)) {
            last = o.i420;
        }
    }
    last
}

const SHOWN_AT: u64 = (T0 + 58_000) as u64;

#[test]
fn the_view_zooms_in_on_the_epicenter_and_returns_to_the_whole_country_when_the_quake_screen_ends() {
    let mut st = Stepper::new(load_renderer(&config(true)).unwrap());
    let ev = chiba_events();
    assert_eq!(st.zoom_ratio(), 1.0);
    run(&mut st, &ev, SHOWN_AT, 20);
    // 千葉県北東部と茨城県南部の予想の範囲に寄る。日本全体よりずっと小さい
    assert!(st.zoom_ratio() > 4.0, "{}", st.zoom_ratio());
    // 3 分後 (地震の画面が終わった平時) は日本全体に戻る
    run(&mut st, &ev, SHOWN_AT + 10 * 60_000, 2);
    assert_eq!(st.zoom_ratio(), 1.0);
}

#[test]
fn zoom_off_keeps_the_whole_country() {
    let mut st = Stepper::new(load_renderer(&config(false)).unwrap());
    run(&mut st, &chiba_events(), SHOWN_AT, 20);
    assert_eq!(st.zoom_ratio(), 1.0);
}

#[test]
fn the_same_frame_times_give_the_same_pictures_regardless_of_the_wall_clock() {
    let ev = chiba_events();
    let a = run(
        &mut Stepper::new(load_renderer(&config(true)).unwrap()),
        &ev,
        SHOWN_AT,
        8,
    );
    std::thread::sleep(std::time::Duration::from_millis(30));
    let b = run(
        &mut Stepper::new(load_renderer(&config(true)).unwrap()),
        &ev,
        SHOWN_AT,
        8,
    );
    assert!(a == b);
}

#[test]
fn a_zoomed_picture_leaves_the_bar_and_the_side_panel_as_they_are() {
    let ev = chiba_events();
    let home = run(
        &mut Stepper::new(load_renderer(&config(false)).unwrap()),
        &ev,
        SHOWN_AT,
        20,
    );
    let zoomed = run(
        &mut Stepper::new(load_renderer(&config(true)).unwrap()),
        &ev,
        SHOWN_AT,
        20,
    );
    let (w, h) = (draw::W as usize, draw::H as usize);
    let y = |pic: &[u8], x: usize, row: usize| pic[row * w + x];
    // 輝度の面 (先頭の w x h バイト) で、上部バーと右パネルは同じ
    let differs = |x: usize, row: usize| y(&home, x, row) != y(&zoomed, x, row);
    let outside_map = |x: usize, row: usize| row < draw::BAR_H as usize || x >= draw::MAP_W as usize;
    assert!(
        !(0..h).any(|row| (0..w).any(|x| outside_map(x, row) && differs(x, row))),
        "地図の外が変わった"
    );
    // 地図の中は変わっている (寄ったので)
    assert!((0..h).any(|row| (0..w).any(|x| !outside_map(x, row) && differs(x, row))));
}

#[test]
fn the_insets_are_not_drawn_while_zoomed_and_come_back_at_home() {
    let mut r = load_renderer(&config(true)).unwrap();
    let e = eew::latest_eews(&chiba_events()).remove(0);
    let icons = Icons::new();
    let scene = Scene {
        quake: None,
        eew: Some(&e),
        history: &[],
        warnings: None,
        weather: None,
        icons: &icons,
        flip_s: 0,
        now_ms: SHOWN_AT,
        connected: true,
        bgm_title: "",
        label: "",
        test: false,
        hindsight: None,
        fast_forward: false,
    };
    // 南西諸島の別枠の縁 (枠の 1 画素外側の線)
    let line = |pm: &tiny_skia::Pixmap| {
        let p = pm.pixel(9, 150).unwrap();
        [p.red(), p.green(), p.blue()]
    };
    assert_eq!(line(&r.render(&scene)), draw::INSET_LINE);
    r.set_view(Some(camera::fit_box(
        camera::MapBox::around(geo::project(140.8, 35.7).0, geo::project(140.8, 35.7).1, 80.0).pad(),
        camera::map_aspect(),
    )));
    assert_ne!(line(&r.render(&scene)), draw::INSET_LINE); // 枠は描かれず、寄った地図 (陸か海) になる
    r.set_view(None);
    assert_eq!(line(&r.render(&scene)), draw::INSET_LINE);
}

/// 寄る・寄らないで 1 コマを描く時間 (リリースビルドで。e2 の配信に寄りを使えるかの見積もり)。
/// 波が広がって表示範囲が毎コマ動く間 (発生の 20 秒後から 8 秒。S 波が揺れた範囲の端へ届くまで) と、落ち着いた後 (60 秒後から 8 秒) を測る。
/// フォントは EQ_NATIVE_FONT (無ければ文字は描かない)。
/// `cargo test --release zoom_cost -- --ignored --nocapture`
#[test]
#[ignore]
fn zoom_cost() {
    const FRAMES: u64 = 40;
    let ev = chiba_events();
    for (phase, from) in [("moving", T0 as u64 + 20_000), ("settled", T0 as u64 + 60_000)] {
        for zoom in [false, true] {
            let mut cfg = config(zoom);
            cfg.font = std::env::var("EQ_NATIVE_FONT").unwrap_or(cfg.font);
            let mut st = Stepper::new(load_renderer(&cfg).unwrap());
            run(&mut st, &ev, from - 2_000, 10); // 温める
            let t = std::time::Instant::now();
            run(&mut st, &ev, from, FRAMES);
            let per = t.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64;
            println!("{phase} zoom={zoom}: {per:.1} ms/frame ({FRAMES} frames at 5fps)");
        }
    }
}

/// 寄った画面を PNG に書く (EQ_NATIVE_PNG_DIR があるときだけ。フォントは EQ_NATIVE_FONT)
#[test]
fn write_zoomed_png_when_asked() {
    let Ok(out) = std::env::var("EQ_NATIVE_PNG_DIR") else {
        return;
    };
    let mut cfg = config(true);
    cfg.font = std::env::var("EQ_NATIVE_FONT").unwrap_or(cfg.font);
    let mut r = load_renderer(&cfg).unwrap();
    let e = eew::latest_eews(&chiba_events()).remove(0);
    let icons = Icons::new();
    let scene = Scene {
        quake: None,
        eew: Some(&e),
        history: &[],
        warnings: None,
        weather: None,
        icons: &icons,
        flip_s: 0,
        now_ms: SHOWN_AT,
        connected: true,
        bgm_title: "",
        label: "",
        test: false,
        hindsight: None,
        fast_forward: false,
    };
    let aim = camera::Aim {
        epicenter: Some(camera::Epicenter {
            lat: 35.7,
            lon: 140.8,
            depth_km: 40.0,
            origin_ms: Some(T0),
        }),
        shaken: r.shaken_box(&["千葉県北東部", "茨城県南部"], &["千葉県", "茨城県"]),
        forecast: true,
    };
    r.set_view(camera::target_box(&aim, SHOWN_AT).map(|b| camera::fit_box(b, camera::map_aspect())));
    let mut pm = r.render(&scene);
    let waves = eew::waves(&[], std::slice::from_ref(&e), SHOWN_AT);
    r.draw_waves(&mut pm, &waves);
    pm.save_png(std::path::Path::new(&out).join("zoomed.png")).unwrap();
}
