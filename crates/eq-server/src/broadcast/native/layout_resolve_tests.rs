//! layout_resolve (定義 → 矩形) のテスト。配信用の定義の割り付けは、今の native の定数と 1px 単位で一致すること。

use super::draw::{H, W};
use super::frame::{Frame, OKINAWA};
use super::geo::View;
use super::layout_def::{parse, pick, LayoutDef};
use super::layout_resolve::{resolve, unsupported_slots, Rect};
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

fn near(a: Rect, b: Rect) {
    for (x, y) in [(a.x, b.x), (a.y, b.y), (a.w, b.w), (a.h, b.h)] {
        assert!((x - y).abs() < 1.0, "{a:?} と {b:?} が 1px 以上ずれている");
    }
}

// 描画 (draw.rs・panel.rs・notice.rs・frame.rs・test_mark.rs) は、この矩形の原点に相対オフセットを足して描く。
// 矩形の値が変わると配信の出力が変わるので、組み込みの定義から割り付けた値を 1px 単位で守る
// (PNG が起点と一致することは、描画の変更のたびに shasum で確かめている)。

#[test]
fn the_calm_layout_has_the_rects_the_drawing_assumes() {
    let m = resolve(&shipped("broadcast"), W as f32, H as f32).unwrap();
    assert_eq!(m["topbar"], r(0.0, 0.0, W as f32, 36.0));
    assert_eq!(m["main"], r(0.0, 36.0, 900.0, 684.0));
    // 右の列 (詳細の区切り線 y180、履歴の見出し y204・1 行目 y216、出典の 1 行目 y640)
    assert_eq!(m["detail"], r(900.0, 36.0, 380.0, 144.0));
    assert_eq!(m["history"], r(900.0, 180.0, 380.0, 312.0));
    // お知らせの箱 (x = 矩形 + 16、y = 矩形の上端 492)
    assert_eq!(m["notice"], r(900.0, 492.0, 380.0, 138.0));
    // 出典 (下端は画面の下端 720。1 行目の字の上端より少し上が 630)
    assert_eq!(m["credit"], r(900.0, 630.0, 380.0, 90.0));
    assert!(!m.contains_key("map-sub"));
}

#[test]
fn the_overlays_have_the_rects_the_drawing_assumes() {
    let m = resolve(&shipped("broadcast"), W as f32, H as f32).unwrap();
    // 別枠 (左上と高さを使い、幅は範囲の縦横比で決まる実数 約 225.9)
    let main = View::fit_home(m["main"].tuple64());
    let (ins, _) = Frame::inset(&main, &OKINAWA, m["inset"]).unwrap().inset_box().unwrap();
    near(m["inset"], r(ins.0, ins.1, ins.2, ins.3));
    assert_eq!((m["inset"].x, m["inset"].y, m["inset"].h), (10.0, 46.0, 220.0));
    // 凡例
    assert_eq!(m["legend"], r(10.0, 563.0, 34.0, 147.0));
    // 時計 (176x74、地図の右下から 10px)
    assert_eq!(m["clock"], r(714.0, 636.0, 176.0, 74.0));
}

#[test]
fn the_quake_layout_has_the_sub_map_rect_and_the_same_frame() {
    let calm = resolve(&shipped("broadcast"), W as f32, H as f32).unwrap();
    let q = resolve(&shipped("broadcast-quake"), W as f32, H as f32).unwrap();
    assert_eq!(q["map-sub"], r(900.0, 190.0, 380.0, 300.0));
    // 枠 (地図・上部バー・重ね物) は平時と同じ。違うと base の絵や投影を作り直すことになる
    for k in ["main", "topbar", "inset", "legend", "clock"] {
        assert_eq!(calm[k], q[k], "{k}");
    }
    assert!(!q.contains_key("notice"));
    // 詳細は平時より 10px 高い (サブの地図を y190 から置くため)。出典は同じ
    assert_eq!(q["detail"], r(900.0, 36.0, 380.0, 154.0));
    assert_eq!(q["credit"], calm["credit"]);
}

#[test]
fn the_drawing_uses_the_quake_rects_only_on_the_quake_screen() {
    use super::placed::Placed;
    let p = Placed::builtin().unwrap();
    let calm = resolve(&shipped("broadcast"), W as f32, H as f32).unwrap();
    let q = resolve(&shipped("broadcast-quake"), W as f32, H as f32).unwrap();
    let c = p.for_screen(false);
    assert_eq!((c.detail, c.history), (Some(calm["detail"]), Some(calm["history"])));
    let s = p.for_screen(true);
    assert_eq!((s.detail, s.history), (Some(q["detail"]), Some(q["history"])));
    // 詳細・サブの地図・履歴は上から順に、重ならず右の列の中に収まる (出典の上まで)
    let (d, m, h) = (s.detail.unwrap(), s.sub.unwrap(), s.history.unwrap());
    assert!(d.bottom() <= m.y && m.bottom() <= h.y && h.bottom() <= q["credit"].y);
    assert_eq!(
        (s.main, s.topbar, s.credit, s.clock),
        (c.main, c.topbar, c.credit, c.clock)
    );
    assert_eq!(p.sub, Some(q["map-sub"]));
    assert_eq!(p.map_aspect(), 900.0 / 684.0);
    assert_eq!(p.sub_aspect(), Some(380.0 / 300.0));
}

/// 右の列の割合 (PR に載せる比較の元の数値。docs/ui-spec/layout-system.js の jquake・jdq の概算と並べる)。
/// 定義の微調整で落ちないよう、値は出すだけで確かめない
#[test]
fn the_column_ratios_of_the_broadcast_layouts() {
    let calm = resolve(&shipped("broadcast"), W as f32, H as f32).unwrap();
    let q = resolve(&shipped("broadcast-quake"), W as f32, H as f32).unwrap();
    println!("平時: 地図 {:.1}%", calm["main"].w / W as f32 * 100.0);
    let col = q["detail"].h + q["map-sub"].h + q["history"].h + q["credit"].h;
    for k in ["detail", "map-sub", "history", "credit"] {
        println!("地震の画面: {k} {:.1}% (列 {col}px)", q[k].h / col * 100.0);
    }
}

#[test]
fn a_quake_layout_whose_right_column_rects_overlap_is_refused() {
    use super::placed::Placed;
    let ok = shipped("broadcast-quake");
    assert!(Placed::new(&ok, &ok).is_ok());
    // 幅 400px の地図の隅に、詳細 (左上) と履歴 (上の中央) を置くと互いに重なる
    let d = def(
        r#"{"dir":"row","children":[{"slot":"main","size":"400px","overlays":{"top-left":{"flow":"column","items":["detail"]},"top":{"flow":"column","items":["history"]}}}]}"#,
    );
    let e = Placed::new(&d, &d).unwrap_err().to_string();
    assert!(e.contains("重なっている"), "{e}");
}

#[test]
fn a_notice_rect_too_short_for_the_box_is_not_drawn() {
    use super::placed::Placed;
    let d = def(r#"{"dir":"column","children":[{"slot":"main"},{"slot":"notice","size":"100px"}]}"#);
    let p = Placed::new(&d, &d).unwrap();
    assert!(p.notice.is_none() && p.detail.is_none() && p.topbar.is_none());
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
fn fill_zero_does_not_produce_nan() {
    let d = def(r#"{"dir":"column","children":[{"slot":"topbar","size":"36px"},{"slot":"main","size":"fill:0"}]}"#);
    assert_eq!(resolve(&d, 1280.0, 720.0).unwrap()["main"].h, 0.0);
}

#[test]
fn a_row_stack_aligns_across_like_the_css_by_the_corner_side() {
    // web/public/style.css: 左の隅は flex-start (上)、右の隅は flex-end (下)、top / bottom は center
    let at = |corner: &str| {
        let d = def(&format!(
            r#"{{"dir":"column","children":[{{"slot":"main","overlays":{{"{corner}":{{"flow":"row","pad":"0","gap":"0","items":["clock","legend"]}}}}}}]}}"#
        ));
        resolve(&d, 1000.0, 500.0).unwrap()
    };
    // 積みの箱の高さは高い方 (凡例 147)。時計 (74) の縦位置だけが変わる
    assert_eq!(at("bottom-left")["clock"].y, 353.0);
    assert_eq!(at("bottom-left")["clock"].y, 500.0 - 147.0);
    assert_eq!(at("top-right")["clock"].y, 147.0 - 74.0);
    assert_eq!(at("top")["clock"].y, (147.0 - 74.0) / 2.0);
    assert_eq!(at("top-left")["clock"].y, 0.0);
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

#[test]
fn a_layout_whose_map_has_no_width_or_falls_off_the_screen_is_refused() {
    let file = parse(
        r#"{"version": 1, "layouts": [
          {"name": "zero", "root": {"dir": "row", "children": [{"slot": "main"}, {"slot": "detail", "size": "1280px"}]}},
          {"name": "over", "root": {"dir": "column", "children": [{"slot": "main", "size": "700px"}, {"slot": "credit", "size": "100px"}]}},
          {"name": "ok", "root": {"dir": "column", "children": [{"slot": "main"}]}}
        ]}"#,
    )
    .unwrap();
    let def = |n: &str| file.layouts.iter().find(|l| l.name == n).unwrap().clone();
    assert!(super::placed::Placed::new(&def("zero"), &def("ok")).is_err());
    assert!(super::placed::Placed::new(&def("over"), &def("ok")).is_err());
    assert!(super::placed::Placed::new(&def("ok"), &def("ok")).is_ok());
}
