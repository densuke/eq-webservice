//! layout_resolve (定義 → 矩形) のテスト。配信用の定義の割り付けは、今の native の定数と 1px 単位で一致すること。

use super::draw::{BAR_H, H, MAP_RECT, MAP_W, SUB_RECT, W};
use super::frame::{Frame, OKINAWA};
use super::geo::View;
use super::layout_def::{parse, pick, LayoutDef};
use super::layout_resolve::{resolve, unsupported_slots, Rect};
use super::panel::LEGEND_RECT;
use crate::layout::BUILTIN;

fn shipped(name: &str) -> LayoutDef {
    pick(&parse(BUILTIN).unwrap(), name).unwrap().clone()
}

fn def(root: &str) -> LayoutDef {
    pick(
        &parse(&format!(r#"{{"version":1,"layouts":[{{"name":"t","root":{root}}}]}}"#)).unwrap(),
        "t",
    )
    .unwrap()
    .clone()
}

fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect { x, y, w, h }
}

fn tuple(c: (f64, f64, f64, f64)) -> Rect {
    r(c.0 as f32, c.1 as f32, c.2 as f32, c.3 as f32)
}

fn near(a: Rect, b: Rect) {
    for (x, y) in [(a.x, b.x), (a.y, b.y), (a.w, b.w), (a.h, b.h)] {
        assert!((x - y).abs() < 1.0, "{a:?} と {b:?} が 1px 以上ずれている");
    }
}

#[test]
fn the_calm_layout_matches_the_current_constants() {
    let m = resolve(&shipped("broadcast"), W as f32, H as f32).unwrap();
    assert_eq!(m["topbar"], r(0.0, 0.0, W as f32, BAR_H));
    assert_eq!(m["main"], tuple(MAP_RECT));
    // 右の列 (panel.rs の SIDE_X = MAP_W、詳細の区切り線 y180、履歴の見出し y204・1 行目 y216、出典の 1 行目 y640)
    assert_eq!(m["detail"], r(MAP_W, BAR_H, 380.0, 144.0));
    assert_eq!(m["history"], r(MAP_W, 180.0, 380.0, 312.0));
    // お知らせの箱 (notice.rs の BOX_X = MAP_W + 16、BOX_Y = 492)
    assert_eq!(m["notice"], r(MAP_W, 492.0, 380.0, 138.0));
    // 出典の一番上 (notice.rs の CREDIT_TOP = H - 5 - 15 * 5 - 10)
    assert_eq!(m["credit"], r(MAP_W, H as f32 - 5.0 - 75.0 - 10.0, 380.0, 90.0));
    assert!(!m.contains_key("map-sub"));
}

#[test]
fn the_overlays_match_the_current_constants() {
    let m = resolve(&shipped("broadcast"), W as f32, H as f32).unwrap();
    // 別枠 (frame.rs の OKINAWA。幅は範囲の縦横比で決まる実数)
    let main = View::fit_home(MAP_RECT);
    let (ins, _) = Frame::inset(&main, &OKINAWA).unwrap().inset_box().unwrap();
    near(m["inset"], r(ins.0, ins.1, ins.2, ins.3));
    assert_eq!(
        (m["inset"].x, m["inset"].y, m["inset"].h),
        (OKINAWA.x, OKINAWA.y, OKINAWA.h)
    );
    // 凡例 (panel.rs の LEGEND_RECT)
    let (x, y, w, h) = LEGEND_RECT;
    assert_eq!(m["legend"], r(x, y, w, h));
    // 時計 (panel.rs の draw_clock: 176x74、地図の右下から 10px)
    assert_eq!(m["clock"], r(MAP_W - 10.0 - 176.0, H as f32 - 10.0 - 74.0, 176.0, 74.0));
}

#[test]
fn the_quake_layout_matches_the_current_constants() {
    let calm = resolve(&shipped("broadcast"), W as f32, H as f32).unwrap();
    let q = resolve(&shipped("broadcast-quake"), W as f32, H as f32).unwrap();
    assert_eq!(q["map-sub"], tuple(SUB_RECT));
    // 枠 (地図・上部バー・重ね物) は平時と同じ。違うと base の絵や投影を作り直すことになる
    for k in ["main", "topbar", "inset", "legend", "clock"] {
        assert_eq!(calm[k], q[k], "{k}");
    }
    assert!(!q.contains_key("notice"));
    // 詳細は平時より 10px 高い (サブの地図を y190 から置くため)。出典は同じ
    assert_eq!(q["detail"], r(MAP_W, BAR_H, 380.0, 154.0));
    assert_eq!(q["credit"], calm["credit"]);
}

#[test]
fn fill_shares_the_rest_by_weight_after_the_fixed_sizes() {
    let d = def(
        r#"{"dir":"row","children":[{"slot":"topbar","size":"100px"},{"slot":"main","size":"fill:2"},{"slot":"detail","size":"fill"}]}"#,
    );
    let m = resolve(&d, 1000.0, 400.0).unwrap();
    assert_eq!(m["topbar"], r(0.0, 0.0, 100.0, 400.0));
    assert_eq!(m["main"], r(100.0, 0.0, 600.0, 400.0));
    assert_eq!(m["detail"], r(700.0, 0.0, 300.0, 400.0));
}

#[test]
fn percent_and_viewport_units_are_computed_on_the_screen() {
    let d = def(
        r#"{"dir":"column","children":[{"slot":"topbar","size":"10%"},{"slot":"detail","size":"10vh"},{"slot":"credit","size":"50vw"},{"slot":"main"}]}"#,
    );
    let m = resolve(&d, 1280.0, 720.0).unwrap();
    assert_eq!(m["topbar"], r(0.0, 0.0, 1280.0, 72.0));
    assert_eq!(m["detail"], r(0.0, 72.0, 1280.0, 72.0));
    assert_eq!(m["credit"], r(0.0, 144.0, 1280.0, 640.0));
    // 残りは 0 以下でも負にしない
    assert_eq!(m["main"], r(0.0, 784.0, 1280.0, 0.0));
}

#[test]
fn auto_is_a_fixed_size_per_part_and_a_container_sums_its_children() {
    let d = def(
        r#"{"dir":"column","children":[{"slot":"topbar","size":"auto"},{"dir":"row","size":"auto","children":[{"slot":"clock","size":"auto"},{"slot":"legend","size":"auto"}]},{"slot":"main"}]}"#,
    );
    let m = resolve(&d, 1280.0, 720.0).unwrap();
    assert_eq!(m["topbar"], r(0.0, 0.0, 1280.0, 36.0));
    // 行の高さは中身 (時計 74・凡例 147) の高い方。幅は並べた分
    assert_eq!(m["clock"], r(0.0, 36.0, 176.0, 147.0));
    assert_eq!(m["legend"], r(176.0, 36.0, 34.0, 147.0));
    assert_eq!(m["main"], r(0.0, 183.0, 1280.0, 537.0));
}

#[test]
fn a_corner_stack_is_pushed_to_its_corner_with_the_pad_and_the_gap() {
    let d = def(r#"{"dir":"column","children":[{"slot":"main","overlays":{
            "top-left":{"flow":"column","gap":"4px","pad":"6px 8px","items":["clock","legend"]}}}]}"#);
    let m = resolve(&d, 1000.0, 500.0).unwrap();
    assert_eq!(m["clock"], r(8.0, 6.0, 176.0, 74.0));
    assert_eq!(m["legend"], r(8.0, 84.0, 34.0, 147.0));
    let d = def(r#"{"dir":"column","children":[{"slot":"main","overlays":{
            "bottom-right":{"flow":"row","pad":"0","gap":"0","items":["legend",{"flow":"column","items":["clock"]}]},
            "top":{"flow":"column","items":["inset"]}}}]}"#);
    let m = resolve(&d, 1000.0, 500.0).unwrap();
    // 右下: 入れ子の積み (176 幅) が右端、その左に凡例。どちらも下端にそろう
    assert_eq!(m["clock"], r(824.0, 426.0, 176.0, 74.0));
    assert_eq!(m["legend"], r(790.0, 353.0, 34.0, 147.0));
    // 上: 横は中央、上から 10px
    assert_eq!(m["inset"], r((1000.0 - 226.0) / 2.0, 10.0, 226.0, 220.0));
}

#[test]
fn parts_the_broadcast_cannot_draw_are_left_out_and_reported() {
    let d = def(
        r#"{"dir":"column","children":[{"slot":"settings","size":"auto"},{"slot":"main","overlays":{"top-left":{"flow":"column","items":["countdown","clock"]}}}]}"#,
    );
    let m = resolve(&d, 1280.0, 720.0).unwrap();
    // 描けない部品は場所も取らない
    assert_eq!(m["main"], r(0.0, 0.0, 1280.0, 720.0));
    assert!(!m.contains_key("settings") && !m.contains_key("countdown"));
    assert_eq!(m["clock"], r(10.0, 10.0, 176.0, 74.0));
    assert_eq!(unsupported_slots(&d), ["settings", "countdown"]);
    assert!(unsupported_slots(&shipped("broadcast")).is_empty());
}
