//! 寄り (zoom) の描画の確認 (実際の地図データで描く。文字は描かない設定で、CI にフォントが無くてもよい)。
//! 2026-10-01 21:27 の千葉県北東部の地震 (緊急地震速報の予想) を使う。

use super::draw::Scene;
use super::*;
use crate::quake::{Eew, EewArea, EventBody, Hypocenter, PrefScale, Scale};

const T0: i64 = 1_790_000_000_000;

fn config(zoom: bool) -> BroadcastConfig {
    config_sub(zoom, false)
}

fn config_sub(zoom: bool, sub_map: bool) -> BroadcastConfig {
    BroadcastConfig {
        sub_map,
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
        status: None,
        notices: None,
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
        status: None,
        notices: None,
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

/// 2026-10-02 04:05 トカラ列島近海: 十島村だけが震度 3 (震度速報は区域で届く)。県の本土 (九州) を映さない
#[test]
fn an_observed_quake_zooms_on_the_shaken_area_not_on_the_prefecture_mainland() {
    use crate::quake::{ObservationPoint, Quake, QuakeInfoType};
    let ev = vec![Event {
        id: "q".into(),
        source: "test".into(),
        received_at_ms: (T0 + 60_000) as u64,
        body: EventBody::Quake(Quake {
            info_type: QuakeInfoType::ScalePrompt,
            origin_time: "2026/10/02 04:05:00".into(),
            origin_time_ms: Some(T0),
            issued_at: "2026/10/02 04:06:00".into(),
            hypocenter: Some(Hypocenter {
                name: "トカラ列島近海".into(),
                latitude: Some(29.5),
                longitude: Some(129.6),
                depth_km: Some(10),
                magnitude: Some(4.0),
            }),
            max_scale: Scale::S3,
            domestic_tsunami: "None".into(),
            points: vec![ObservationPoint {
                pref: "鹿児島県".into(),
                addr: "鹿児島県十島村".into(),
                is_area: true,
                scale: Scale::S3,
                station: None,
            }],
            pref_max: vec![PrefScale {
                pref: "鹿児島県".into(),
                scale: Scale::S3,
            }],
            comment: String::new(),
        }),
    }];
    let mut st = Stepper::new(load_renderer(&config(true)).unwrap());
    run(&mut st, &ev, (T0 + 200_000) as u64, 20); // 波が終わって落ち着いた範囲
    let f = st.zoom_fit();
    let (x, y) = geo::project(130.55, 31.6); // 鹿児島市
    assert!(st.zoom_ratio() > 4.0);
    assert!(
        !(f.x..=f.x + f.w).contains(&x) || !(f.y..=f.y + f.h).contains(&y),
        "{f:?}"
    );
    let (tx, ty) = geo::project(129.6, 29.5);
    assert!((f.x..=f.x + f.w).contains(&tx) && (f.y..=f.y + f.h).contains(&ty));
}

/// 2026-10-02 17:39 熊本県熊本地方: 緊急地震速報 (震度 3 の予想は区域を持たない) の第 1〜5 報のあと、
/// 発生から 145 秒で震度速報 (熊本県熊本が震度 3) が届く。波が続いている 180 秒より前でも、震度速報が届いたら熊本の区域へ寄る
fn kumamoto_events(eew_areas: &[EewArea]) -> Vec<Event> {
    use crate::quake::{ObservationPoint, Quake, QuakeInfoType};
    let hypo = || Hypocenter {
        name: "熊本県熊本地方".into(),
        latitude: Some(32.8),
        longitude: Some(130.7),
        depth_km: Some(10),
        magnitude: Some(3.5),
    };
    let mut ev: Vec<Event> = [8u64, 9, 11, 31, 36]
        .iter()
        .enumerate()
        .map(|(i, s)| Event {
            id: format!("e-{}", i + 1),
            source: "test".into(),
            received_at_ms: T0 as u64 + s * 1000,
            body: EventBody::Eew(Eew {
                event_id: "k".into(),
                serial: (i + 1).to_string(),
                cancelled: false,
                test: false,
                warning: false,
                issued_at: "2026/10/02 17:39:30".into(),
                origin_time: Some("2026/10/02 17:39:22".into()),
                origin_time_ms: Some(T0),
                hypocenter: Some(hypo()),
                areas: eew_areas.to_vec(),
                pref_max: vec![],
                max_scale: Scale::S3,
            }),
        })
        .collect();
    ev.push(Event {
        id: "q".into(),
        source: "test".into(),
        received_at_ms: T0 as u64 + 145_000,
        body: EventBody::Quake(Quake {
            info_type: QuakeInfoType::ScalePrompt,
            origin_time: "2026/10/02 17:39:00".into(),
            origin_time_ms: Some(T0),
            issued_at: "2026/10/02 17:41:30".into(),
            hypocenter: Some(hypo()),
            max_scale: Scale::S3,
            domestic_tsunami: "None".into(),
            points: vec![ObservationPoint {
                pref: "熊本県".into(),
                addr: "熊本県熊本".into(),
                is_area: true,
                scale: Scale::S3,
                station: None,
            }],
            pref_max: vec![PrefScale {
                pref: "熊本県".into(),
                scale: Scale::S3,
            }],
            comment: String::new(),
        }),
    });
    ev
}

fn at(s: u64) -> u64 {
    T0 as u64 + s * 1000
}

/// 発生から s 秒までに届いた報だけで、a 秒から n コマ進めて、寄った表示の幅を返す
fn width_after(ev: &[Event], st: &mut Stepper, s: u64, from: u64, n: u64) -> f64 {
    let seen: Vec<Event> = ev.iter().filter(|e| e.received_at_ms <= at(s)).cloned().collect();
    run(st, &seen, at(from), n);
    st.zoom_fit().w
}

#[test]
fn a_scale_prompt_zooms_in_on_its_area_while_the_waves_are_still_drawn() {
    let ev = kumamoto_events(&[]);
    let mut st = Stepper::new(load_renderer(&config(true)).unwrap());
    let before = width_after(&ev, &mut st, 140, 132, 40);
    let after = width_after(&ev, &mut st, 150, 146, 60);
    assert!(after < before / 2.0, "{before} -> {after}");
}

/// 緊急地震速報の予想の区域 (阿蘇・球磨) も、震度速報が届いたあとの寄りに収める
#[test]
fn the_forecast_areas_of_the_eew_stay_in_the_view_after_the_scale_prompt() {
    let wide = [
        area("熊本県阿蘇", Scale::S3, Scale::S3),
        area("熊本県球磨", Scale::S3, Scale::S3),
    ];
    let narrow = width_after(
        &kumamoto_events(&[]),
        &mut Stepper::new(load_renderer(&config(true)).unwrap()),
        150,
        146,
        60,
    );
    let with_eew = width_after(
        &kumamoto_events(&wide),
        &mut Stepper::new(load_renderer(&config(true)).unwrap()),
        150,
        146,
        60,
    );
    assert!(with_eew > narrow * 1.1, "{narrow} vs {with_eew}");
}

/// 再現動画: 本物の震源が届くまでは、のちに分かった震源 (薄い印) へ寄る
#[test]
fn the_pending_hindsight_epicenter_is_zoomed_to_until_the_real_one_arrives() {
    let h = Hindsight {
        lat: 35.7,
        lon: 140.8,
        depth_km: 40.0,
        origin_ms: T0,
    };
    let icons = Icons::new();
    let mut st = Stepper::new(load_renderer(&config(true)).unwrap());
    for k in 0..20 {
        let i = Input {
            hindsight: Some(&h),
            ..input(&[], (T0 + 5_000) as u64 + k * 200, &icons)
        };
        st.step(&i);
    }
    assert!(st.zoom_ratio() > 4.0, "{}", st.zoom_ratio());
    // 報も震源も無く、のちに分かった震源も無くなれば日本全体へ戻る
    run(&mut st, &[], (T0 + 600_000) as u64, 2);
    assert_eq!(st.zoom_ratio(), 1.0);
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
        status: None,
        notices: None,
    };
    let aim = camera::Aim {
        epicenter: Some(camera::Epicenter {
            lat: 35.7,
            lon: 140.8,
            depth_km: 40.0,
            origin_ms: Some(T0),
        }),
        shaken: r
            .zones()
            .shaken(&["千葉県北東部", "茨城県南部"], &[], &["千葉県", "茨城県"]),
        forecast: true,
    };
    r.set_view(camera::target_box(&aim, SHOWN_AT).map(|b| camera::fit_box(b, camera::map_aspect())));
    let mut pm = r.render(&scene);
    let waves = eew::waves(&[], std::slice::from_ref(&e), SHOWN_AT);
    r.draw_waves(&mut pm, &waves);
    pm.save_png(std::path::Path::new(&out).join("zoomed.png")).unwrap();
}

// ---- sub_map (配信の右パネルの上にサブの地図を描く試験。docs/native-submap-bench.md) ----

fn eew_scene<'a>(e: &'a eew::EewSummary, icons: &'a Icons, now_ms: u64) -> Scene<'a> {
    Scene {
        quake: None,
        eew: Some(e),
        history: &[],
        warnings: None,
        weather: None,
        icons,
        flip_s: 0,
        now_ms,
        connected: true,
        bgm_title: "",
        label: "",
        test: false,
        hindsight: None,
        fast_forward: false,
        status: None,
        notices: None,
    }
}

fn in_sub_rect(i: usize) -> bool {
    let (x, y) = ((i % draw::W as usize) as f64, (i / draw::W as usize) as f64);
    let (rx, ry, rw, rh) = draw::SUB_RECT;
    (rx..rx + rw).contains(&x) && (ry..ry + rh).contains(&y)
}

#[test]
fn the_sub_map_paints_only_inside_its_rect() {
    let e = eew::latest_eews(&chiba_events()).remove(0);
    let icons = Icons::new();
    let scene = eew_scene(&e, &icons, SHOWN_AT);
    let off = load_renderer(&config_sub(false, false)).unwrap().render(&scene);
    let mut r = load_renderer(&config_sub(false, true)).unwrap();
    assert!(r.sub_map_enabled());
    let (x, y) = geo::project(140.8, 35.7);
    let aspect = draw::SUB_RECT.2 / draw::SUB_RECT.3;
    r.set_sub_view(Some(camera::fit_box(camera::MapBox::around(x, y, 150.0), aspect)));
    let on = r.render(&scene);
    let (mut outside, mut inside) = (0, 0);
    for (i, (a, b)) in off.pixels().iter().zip(on.pixels()).enumerate() {
        match (a == b, in_sub_rect(i)) {
            (false, false) => outside += 1,
            (false, true) => inside += 1,
            _ => {}
        }
    }
    assert_eq!(outside, 0);
    assert!(inside > 0);
}

#[test]
fn the_sub_map_does_not_add_redraws() {
    let ev = chiba_events();
    let icons = Icons::new();
    for zoom in [false, true] {
        let count = |sub| {
            let mut st = Stepper::new(load_renderer(&config_sub(zoom, sub)).unwrap());
            (0..50)
                .filter(|k| st.step(&input(&ev, SHOWN_AT + k * 200, &icons)).is_some())
                .count()
        };
        assert_eq!(count(true), count(false), "zoom={zoom}");
    }
}

fn quake_event(received_ms: u64) -> Event {
    use crate::quake::{Quake, QuakeInfoType};
    Event {
        id: "q".into(),
        source: "test".into(),
        received_at_ms: received_ms,
        body: EventBody::Quake(Quake {
            info_type: QuakeInfoType::ScaleAndDestination,
            origin_time: "2026/10/01 21:26:50".into(),
            origin_time_ms: Some(T0),
            issued_at: "2026/10/01 21:30:00".into(),
            hypocenter: Some(Hypocenter {
                name: "千葉県北東部".into(),
                latitude: Some(35.7),
                longitude: Some(140.8),
                depth_km: Some(40),
                magnitude: Some(4.5),
            }),
            max_scale: Scale::S4,
            domestic_tsunami: "None".into(),
            points: vec![],
            pref_max: vec![PrefScale {
                pref: "千葉県".into(),
                scale: Scale::S4,
            }],
            comment: String::new(),
        }),
    }
}

#[test]
fn the_sub_map_shows_the_latest_quake_when_calm() {
    let received = (T0 + 60_000) as u64;
    let ev = [quake_event(received)];
    let icons = Icons::new();
    // 落ち着きの時間 (3 分) を過ぎた後 = 平時
    let now = received + 10 * 60_000;
    let s4 = model::scale_color(Scale::S4);
    let count = |sub| {
        let mut st = Stepper::new(load_renderer(&config_sub(false, sub)).unwrap());
        let o = st.step(&input(&ev, now, &icons)).unwrap();
        assert!(o.calm);
        let pm = st.still_pixmap().unwrap();
        pm.pixels()
            .iter()
            .enumerate()
            .filter(|&(i, p)| in_sub_rect(i) && [p.red(), p.green(), p.blue()] == s4)
            .count()
    };
    let (on, off) = (count(true), count(false));
    assert!(on > off + 500, "{on} {off}");
}

#[test]
fn sub_map_is_read_from_the_config_and_defaults_to_off() {
    assert!(
        !toml::from_str::<BroadcastConfig>("source = \"native\"")
            .unwrap()
            .sub_map
    );
    assert!(
        toml::from_str::<BroadcastConfig>("source = \"native\"\nsub_map = true")
            .unwrap()
            .sub_map
    );
    assert!(!BroadcastConfig::default().sub_map);
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

/// 昇順に並べた時間 (ms) の (中央値, p95, 平均)
fn summarize_ms(mut v: Vec<f64>) -> (f64, f64, f64) {
    v.sort_by(f64::total_cmp);
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    (percentile(&v, 0.5), percentile(&v, 0.95), mean)
}

/// サブの地図の重さ (リリースビルドで。n2 の配信に足せるかの見積もり。docs/native-submap-bench.md)。
/// 場面 (平時・緊急地震速報・確定の地震) x zoom x sub_map で、render() 200 回と step() 100 回 (5fps の時刻) を 1 回ずつ計る。
/// 地図は EQ_NATIVE_MAP_DIR、フォントは EQ_NATIVE_FONT で指す (無ければ web/public と、文字なし)。
/// `cargo test --release -p eq-server sub_map_cost -- --ignored --nocapture --test-threads=1`
#[test]
#[ignore]
fn sub_map_cost() {
    const RENDERS: usize = 200;
    const STEPS: u64 = 100;
    let received = (T0 + 60_000) as u64;
    let scenes: [(&str, Vec<Event>, u64); 3] = [
        ("calm ", vec![quake_event(received)], received + 10 * 60_000),
        ("eew  ", chiba_events(), T0 as u64 + 20_000),
        ("quake", vec![quake_event(received)], received + 10_000),
    ];
    let icons = Icons::new();
    for (phase, ev, from) in &scenes {
        for zoom in [false, true] {
            for sub in [false, true] {
                let mut cfg = config_sub(zoom, sub);
                if let Ok(d) = std::env::var("EQ_NATIVE_MAP_DIR") {
                    cfg.map_dir = d;
                }
                cfg.font = std::env::var("EQ_NATIVE_FONT").unwrap_or(cfg.font);
                let mut st = Stepper::new(load_renderer(&cfg).unwrap());
                for k in 0..10 {
                    st.step(&input(ev, from + k * 200, &icons)); // 温める (寄りとサブの範囲も入る)
                }
                // render() 単体 (最後に入れた表示範囲のまま)
                let groups = model::group_quakes(ev);
                let eews = eew::latest_eews(ev);
                let current = eew::current(&groups, &eews, from + 2_000);
                let (quake, shown) = match current {
                    Some(eew::Current::Quake(q)) => (Some(q), None),
                    Some(eew::Current::Eew(e)) => (None, Some(e)),
                    None => (None, None),
                };
                let scene = Scene {
                    quake,
                    eew: shown,
                    history: &groups,
                    warnings: None,
                    weather: None,
                    icons: &icons,
                    flip_s: 0,
                    now_ms: from + 2_000,
                    connected: true,
                    bgm_title: "",
                    label: "",
                    test: false,
                    hindsight: None,
                    fast_forward: false,
                    status: None,
                    notices: None,
                };
                let renders: Vec<f64> = (0..RENDERS)
                    .map(|_| {
                        let t = std::time::Instant::now();
                        std::hint::black_box(st.renderer_mut().render(&scene));
                        t.elapsed().as_secs_f64() * 1000.0
                    })
                    .collect();
                let steps: Vec<f64> = (0..STEPS)
                    .map(|k| {
                        let i = input(ev, from + k * 200, &icons);
                        let t = std::time::Instant::now();
                        std::hint::black_box(st.step(&i));
                        t.elapsed().as_secs_f64() * 1000.0
                    })
                    .collect();
                let (rm, rp, _) = summarize_ms(renders);
                let (sm, sp, mean) = summarize_ms(steps);
                println!(
                    "{phase} zoom={zoom:<5} sub={sub:<5}: render med {rm:.2} p95 {rp:.2} / step med {sm:.2} p95 {sp:.2} mean {mean:.2} ms, core-s per video-s {:.4}",
                    mean * 5.0 / 1000.0
                );
            }
        }
    }
}
