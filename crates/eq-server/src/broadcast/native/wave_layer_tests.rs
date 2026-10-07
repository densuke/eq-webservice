//! 地震波の円の重なり順の確認 (#200: 地図 -> 震度の塗り -> 海岸線 -> 波 -> 札・時計・凡例)。

use super::draw::{Layers, Scene};
use super::eew::Wave;
use super::layout_resolve::Rect;
use super::zoom_tests::{chiba_events, config_sub, eew_scene, in_rect, SHOWN_AT};
use super::*;

/// 画面いっぱいを覆う円 (S 波の内側の薄い塗りが、時計・凡例の上にもかかる)
const HUGE: [Wave; 1] = [Wave {
    lat: 35.0,
    lon: 137.0,
    p_km: Some(2800.0),
    s_km: Some(3000.0),
}];

fn layers(zoom: bool) -> (Renderer, Layers, tiny_skia::Pixmap) {
    let e = eew::latest_eews(&chiba_events()).remove(0);
    let icons = Icons::new();
    let scene: Scene = eew_scene(&e, &icons, SHOWN_AT);
    let mut r = load_renderer(&config_sub(zoom, false)).unwrap();
    if zoom {
        let (x, y) = geo::project(140.8, 35.7);
        r.set_view(Some(camera::fit_box(
            camera::MapBox::around(x, y, 150.0),
            r.map_aspect(),
        )));
    }
    let flat = r.render(&scene);
    let l = r.render_layers(&scene);
    (r, l, flat)
}

fn changed_in(a: &tiny_skia::Pixmap, b: &tiny_skia::Pixmap, rect: Rect) -> usize {
    a.pixels()
        .iter()
        .zip(b.pixels())
        .enumerate()
        .filter(|&(i, (p, q))| p != q && in_rect(i, rect))
        .count()
}

#[test]
fn the_clock_and_the_legend_stay_above_the_waves() {
    let p = Placed::builtin().unwrap();
    // 角丸の外 (四隅) は箱ではなく地図なので、四隅を除いた内側で見る
    let inner = |r: Rect| Rect {
        x: r.x + 3.0,
        y: r.y + 3.0,
        w: r.w - 6.0,
        h: r.h - 6.0,
    };
    let (clock, legend) = (inner(p.clock.unwrap()), inner(p.legend.unwrap()));
    for zoom in [false, true] {
        let (r, l, flat) = layers(zoom);
        // 前提: 波を後から重ねる今までの描き方では、時計も凡例も円に塗られる
        let mut over = flat.clone();
        r.draw_waves(&mut over, &HUGE);
        assert!(changed_in(&flat, &over, clock) > 0 && changed_in(&flat, &over, legend) > 0);
        // 波を挟む描き方では、円の有無で時計・凡例の画素が変わらない。地図の中は変わる
        let (calm, waved) = (r.waved(&l, &[], None), r.waved(&l, &HUGE, None));
        assert_eq!(changed_in(&calm, &waved, clock), 0, "zoom={zoom}: 時計");
        assert_eq!(changed_in(&calm, &waved, legend), 0, "zoom={zoom}: 凡例");
        assert!(
            changed_in(&calm, &waved, p.main) > 0,
            "zoom={zoom}: 円が地図に出ていない"
        );
    }
}

/// 波を挟まずに 2 枚を重ねるだけなら、1 枚で描いた絵と (半透明の丸め以外は) 同じ
#[test]
fn the_two_layers_add_up_to_the_flat_picture() {
    for zoom in [false, true] {
        let (r, l, flat) = layers(zoom);
        let joined = r.waved(&l, &[], None);
        let worst = flat
            .data()
            .iter()
            .zip(joined.data())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(worst <= 3, "zoom={zoom}: 最大の差 {worst}");
    }
}

// ---- サブの地図の波 (#199) ----

/// サブの地図を有効にし、震央 (140.8E 35.7N) の周り half (地図の座標) を映す
fn sub_renderer(half: f64) -> (Renderer, Scene<'static>) {
    let e: &'static eew::EewSummary = Box::leak(Box::new(eew::latest_eews(&chiba_events()).remove(0)));
    let icons: &'static Icons = Box::leak(Box::new(Icons::new()));
    let mut cfg = config_sub(false, true);
    cfg.font = std::env::var("EQ_NATIVE_FONT").unwrap_or(cfg.font);
    let mut r = load_renderer(&cfg).unwrap();
    let (x, y) = geo::project(140.8, 35.7);
    let aspect = r.sub_aspect().unwrap();
    r.set_sub_view(Some(camera::fit_box(camera::MapBox::around(x, y, half), aspect)));
    (r, eew_scene(e, icons, SHOWN_AT))
}

/// 震央の周りの P 波の輪 (km)
fn ring(km: f64) -> Wave {
    Wave {
        lat: 35.7,
        lon: 140.8,
        p_km: Some(km),
        s_km: None,
    }
}

type Bounds = Option<(usize, usize, usize, usize)>;

/// 2 枚の絵で違う画素の外接矩形 (x0, y0, x1, y1) と、サブの地図の外で違う数
fn diff_box(a: &tiny_skia::Pixmap, b: &tiny_skia::Pixmap) -> (Bounds, usize) {
    let w = draw::W as usize;
    let (mut bx, mut outside) = (None::<(usize, usize, usize, usize)>, 0);
    for (i, (p, q)) in a.pixels().iter().zip(b.pixels()).enumerate() {
        if p == q {
            continue;
        }
        if !super::zoom_tests::in_sub_rect(i) {
            outside += 1;
        }
        let (x, y) = (i % w, i / w);
        bx = Some(bx.map_or((x, y, x, y), |(a, b, c, d)| (a.min(x), b.min(y), c.max(x), d.max(y))));
    }
    (bx, outside)
}

#[test]
fn the_sub_map_rings_stay_inside_its_rect_and_keep_its_frame() {
    let (mut r, scene) = sub_renderer(150.0);
    let l = r.render_layers(&scene);
    let sub = Placed::builtin().unwrap().sub.unwrap();
    // 矩形より大きな円: 外 (地図の枠・詳細・履歴) は変わらず、中は変わる。1px の枠線も変わらない
    let without = r.waved(&l, &[], None);
    let with = r.waved(&l, &[], Some(&HUGE[0]));
    let (changed, outside) = diff_box(&without, &with);
    assert!(changed.is_some(), "サブの地図に円が出ていない");
    assert_eq!(outside, 0, "サブの地図の外が変わった");
    let rim = |x: f32, y: f32, w: f32, h: f32| Rect { x, y, w, h };
    for edge in [
        rim(sub.x, sub.y, sub.w, 1.0),
        rim(sub.x, sub.bottom() - 1.0, sub.w, 1.0),
        rim(sub.x, sub.y, 1.0, sub.h),
        rim(sub.right() - 1.0, sub.y, 1.0, sub.h),
    ] {
        assert_eq!(changed_in(&without, &with, edge), 0, "枠線");
    }
}

/// 円はサブの寄りの座標で映る: 震央を中心に、映す範囲を 2 倍にすると輪の直径は半分になる
#[test]
fn a_sub_map_ring_follows_the_sub_zoom() {
    let sub = Placed::builtin().unwrap().sub.unwrap();
    let width = |half: f64| {
        let (mut r, scene) = sub_renderer(half);
        let l = r.render_layers(&scene);
        let (changed, _) = diff_box(&r.waved(&l, &[], None), &r.waved(&l, &[], Some(&ring(30.0))));
        let (x0, _, x1, _) = changed.expect("輪が出ていない");
        // 輪は震央 (サブの地図の中心) の周りにある
        let centre = (x0 + x1) as f32 / 2.0;
        assert!((centre - (sub.x + sub.w / 2.0)).abs() < 2.0, "中心 {centre}");
        (x1 - x0) as f64
    };
    let (near, far) = (width(150.0), width(300.0));
    assert!((near / far - 2.0).abs() < 0.1, "{near} / {far}");
}

/// 波の入った画面を PNG に書く (EQ_NATIVE_PNG_DIR があるときだけ。フォントは EQ_NATIVE_FONT)
#[test]
fn write_sub_wave_pngs_when_asked() {
    let Ok(out) = std::env::var("EQ_NATIVE_PNG_DIR") else {
        return;
    };
    let (mut r, scene) = sub_renderer(150.0);
    let l = r.render_layers(&scene);
    for (name, wave) in [("sub_ring", ring(60.0)), ("huge_over_clock_and_legend", HUGE[0])] {
        r.waved(&l, &[wave], Some(&wave))
            .save_png(std::path::Path::new(&out).join(format!("{name}.png")))
            .unwrap();
    }
}
