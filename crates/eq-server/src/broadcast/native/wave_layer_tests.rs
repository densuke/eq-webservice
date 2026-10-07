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
        let (calm, waved) = (r.waved(&l, &[]), r.waved(&l, &HUGE));
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
        let joined = r.waved(&l, &[]);
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
