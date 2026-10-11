//! 観測点の点 (#212): 描く・順序・予測では描かない

use super::geo::View;
use super::model::scale_color;
use super::paint::SEA;
use super::shaken::Point;
use super::tests::{eew, map_rect, quake, renderer, rgb, scene};
use super::text::Text;
use crate::quake::Scale;

/// 日本海 (陸の塗りに紛れない)
const AT: (f64, f64) = (134.0, 40.5);

fn point(addr: &str, scale: Scale) -> Point {
    Point {
        addr: addr.into(),
        is_area: false,
        scale,
        station: Some((AT.1, AT.0, "区域".into())),
    }
}

fn spot() -> (u32, u32) {
    let (x, y) = View::fit_home(map_rect()).px(AT.0, AT.1);
    (x as u32, y as u32)
}

#[test]
fn observation_points_are_dots_in_the_scale_colour_and_the_stronger_one_is_on_top() {
    let mut r = renderer(Text::none());
    let mut g = quake(Scale(40), &[("石川県", Scale(40))], None);
    // 強い点を先に並べても、弱い点が下に描かれる。区域は位置が無いので描かない
    g.points = vec![
        point("強", Scale::S7),
        point("弱", Scale::S1),
        Point {
            is_area: true,
            ..point("区域", Scale::S3)
        },
    ];
    let pm = r.render(&scene(Some(&g), &[], None, None));
    assert_eq!(rgb(&pm, spot()), scale_color(Scale::S7));
    // 点が無ければ海のまま
    g.points.clear();
    let pm = r.render(&scene(Some(&g), &[], None, None));
    assert_eq!(rgb(&pm, spot()), SEA);
}

#[test]
fn forecast_draws_no_dots() {
    let mut r = renderer(Text::none());
    let e = eew(false, &[("石川県", Scale(40))], None);
    let mut s = scene(None, &[], None, None);
    s.eew = Some(&e);
    let pm = r.render(&s);
    assert_eq!(rgb(&pm, spot()), SEA);
}
