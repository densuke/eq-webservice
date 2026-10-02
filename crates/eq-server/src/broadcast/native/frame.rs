//! 地図を映す面 (本図と、離島の別枠)。path は本図の座標で 1 度だけ作り、別枠は変換をかけて映す
//! (別枠は枠の外に描かない)。web/src/map.ts の INSETS と同じ。

use std::sync::Arc;

use tiny_skia::{FillRule, Mask, Path, PathBuilder, PathSegment, Pixmap, Point, Rect, Stroke, Transform};

use super::draw::MAP_RECT;
use super::geo::{project, View};
use super::paint::paint;

/// 別枠の定義 (経度・緯度の範囲と、置き場所)
pub struct InsetSpec {
    pub title: &'static str,
    pub lon: (f64, f64),
    pub lat: (f64, f64),
    /// 枠の左上 (画面の座標) と高さ。幅は範囲の縦横比から決める
    pub x: f32,
    pub y: f32,
    pub h: f32,
}

/// 南西諸島。日本全体の表示から外れる離島を、地図の左上に別枠で映す
pub const OKINAWA: InsetSpec = InsetSpec {
    title: "南西諸島",
    lon: (122.5, 131.5),
    lat: (24.0, 31.0),
    x: 10.0,
    y: super::draw::BAR_H + 10.0,
    h: 220.0,
};

/// 別枠の範囲の外でも、この度数以内の震央は枠の縁に寄せて印を置く / そのときの枠の縁からの余白
const MARKER_MARGIN_DEG: f64 = 1.5;
const MARKER_PAD: f32 = 8.0;

/// 枠 (x, y, 幅, 高さ)
pub type BoxRect = (f32, f32, f32, f32);

pub struct Frame {
    /// この面の経度・緯度 -> 画面の座標
    pub view: View,
    /// 本図 (日本全体) の path をこの面に映す変換 (日本全体の本図は何もしない)
    ts: Transform,
    /// 描いてよい枠。別枠と、寄った本図 (地図の枠の外の上部バーや右パネルに描かない)。日本全体の本図は無し
    clip: Option<Clip>,
    /// 別枠だけ: (枠、範囲 lon0 lon1 lat0 lat1、題)
    inset: Option<Inset>,
}

/// 描いてよい枠と、その外を隠す型 (作るのが重いので、使い回す)
#[derive(Clone)]
pub struct Clip {
    mask: Arc<Mask>,
    rect: Rect,
}

impl Clip {
    fn new(rect: Rect) -> Option<Clip> {
        let mut mask = Mask::new(super::draw::W, super::draw::H)?;
        mask.fill_path(
            &PathBuilder::from_rect(rect),
            FillRule::Winding,
            false,
            Transform::identity(),
        );
        Some(Clip {
            mask: Arc::new(mask),
            rect,
        })
    }

    fn contains(&self, (x, y): (f32, f32)) -> bool {
        (self.rect.left()..=self.rect.right()).contains(&x) && (self.rect.top()..=self.rect.bottom()).contains(&y)
    }
}

struct Inset {
    rect: Rect,
    bounds: (f64, f64, f64, f64),
    title: &'static str,
}

impl Frame {
    pub fn main(view: View) -> Frame {
        Frame {
            view,
            ts: Transform::identity(),
            clip: None,
            inset: None,
        }
    }

    /// 寄った本図の枠 (地図の枠)
    pub fn map_clip() -> Option<Clip> {
        let (x, y, w, h) = MAP_RECT;
        Clip::new(Rect::from_xywh(x as f32, y as f32, w as f32, h as f32)?)
    }

    /// 日本全体の本図 home の path を、view に寄せて映す本図。地図の枠の外には描かない
    pub fn zoomed(home: &View, view: View, clip: &Clip) -> Frame {
        Frame {
            ts: view.transform_from(home),
            view,
            clip: Some(clip.clone()),
            inset: None,
        }
    }

    pub fn inset(main: &View, spec: &InsetSpec) -> Option<Frame> {
        let (x0, y0) = project(spec.lon.0, spec.lat.1);
        let (x1, y1) = project(spec.lon.1, spec.lat.0);
        let w = spec.h * ((x1 - x0) / (y1 - y0)) as f32;
        let rect = Rect::from_xywh(spec.x, spec.y, w, spec.h)?;
        let view = View::fit(
            (spec.lon.0, spec.lon.1, spec.lat.0, spec.lat.1),
            (spec.x as f64, spec.y as f64, w as f64, spec.h as f64),
        );
        Some(Frame {
            ts: view.transform_from(main),
            view,
            clip: Some(Clip::new(rect)?),
            inset: Some(Inset {
                rect,
                bounds: (spec.lon.0, spec.lon.1, spec.lat.0, spec.lat.1),
                title: spec.title,
            }),
        })
    }

    /// 別枠か
    pub fn is_inset(&self) -> bool {
        self.inset.is_some()
    }

    /// 別枠の枠 (x, y, 幅, 高さ) と題
    pub fn inset_box(&self) -> Option<(BoxRect, &'static str)> {
        let i = self.inset.as_ref()?;
        Some(((i.rect.x(), i.rect.y(), i.rect.width(), i.rect.height()), i.title))
    }

    /// その地点をこの面に描くか (本図は全部、別枠は範囲の中だけ)
    pub fn contains(&self, lon: f64, lat: f64) -> bool {
        self.within(lon, lat, 0.0)
    }

    /// 範囲を margin 度広げて見たとき、その地点が別枠の中か (本図は全部)
    fn within(&self, lon: f64, lat: f64, margin: f64) -> bool {
        self.inset.as_ref().is_none_or(|i| {
            (i.bounds.0 - margin..=i.bounds.1 + margin).contains(&lon)
                && (i.bounds.2 - margin..=i.bounds.3 + margin).contains(&lat)
        })
    }

    /// 震央の印を置く画面の位置。別枠の範囲のすぐ外 (南西諸島の少し西の海など) の震央は、枠の縁に寄せて置く。
    /// この面に置かないときは None
    pub fn marker(&self, lon: f64, lat: f64) -> Option<(f32, f32)> {
        if !self.within(lon, lat, MARKER_MARGIN_DEG) {
            return None;
        }
        let (x, y) = self.view.px(lon, lat);
        match &self.inset {
            Some(i) => Some((
                x.clamp(i.rect.left() + MARKER_PAD, i.rect.right() - MARKER_PAD),
                y.clamp(i.rect.top() + MARKER_PAD, i.rect.bottom() - MARKER_PAD),
            )),
            // 寄った本図は、枠の外 (右パネルや上部バーの上) には置かない
            None => self.clip.as_ref().is_none_or(|c| c.contains((x, y))).then_some((x, y)),
        }
    }

    /// 日本全体の本図の画面の座標 (県の札の位置など) を、この面の画面の座標にする。
    /// 寄った本図で枠の外になるときは None
    pub fn point(&self, (x, y): (f32, f32)) -> Option<(f32, f32)> {
        let p = (x * self.ts.sx + self.ts.tx, y * self.ts.sy + self.ts.ty);
        match (&self.clip, &self.inset) {
            (Some(c), None) if !c.contains(p) => None,
            _ => Some(p),
        }
    }

    /// この path が面の中に見えるか (見えないものは描かずに済ませる)
    pub fn sees(&self, path: &Path) -> bool {
        let Some(c) = &self.clip else { return true };
        path.bounds().transform(self.ts).is_some_and(|b| {
            b.left() < c.rect.right()
                && b.right() > c.rect.left()
                && b.top() < c.rect.bottom()
                && b.bottom() > c.rect.top()
        })
    }

    /// この path の外接矩形 (画面の座標)
    pub fn screen_bounds(&self, path: &Path) -> Option<BoxRect> {
        let b = path.bounds().transform(self.ts)?;
        Some((b.left(), b.top(), b.width(), b.height()))
    }

    /// 画面の点 p が path (偶奇の塗り) の内側か。path は本図の座標なので、点を戻してから数える
    pub fn path_contains(&self, path: &Path, p: (f32, f32)) -> bool {
        let (x, y) = ((p.0 - self.ts.tx) / self.ts.sx, (p.1 - self.ts.ty) / self.ts.sy);
        let mut inside = false;
        let mut cross = |a: Point, b: Point| {
            if (a.y > y) != (b.y > y) && x < (b.x - a.x) * (y - a.y) / (b.y - a.y) + a.x {
                inside = !inside;
            }
        };
        let (mut start, mut prev) = (None::<Point>, None::<Point>);
        for seg in path.segments() {
            match seg {
                PathSegment::MoveTo(p) => {
                    if let (Some(a), Some(s)) = (prev, start) {
                        cross(a, s);
                    }
                    (start, prev) = (Some(p), Some(p));
                }
                PathSegment::LineTo(p) | PathSegment::QuadTo(_, p) | PathSegment::CubicTo(_, _, p) => {
                    if let Some(a) = prev {
                        cross(a, p);
                    }
                    prev = Some(p);
                }
                PathSegment::Close => {}
            }
        }
        if let (Some(a), Some(s)) = (prev, start) {
            cross(a, s);
        }
        inside
    }

    pub fn fill(&self, pm: &mut Pixmap, path: &Path, c: [u8; 3], a: f32) {
        if self.sees(path) {
            let mask = self.clip.as_ref().map(|c| &*c.mask);
            pm.fill_path(path, &paint(c, a), FillRule::EvenOdd, self.ts, mask);
        }
    }

    /// 線の太さ width は画面での太さ (別枠・寄った本図でも同じ太さに見える)
    pub fn stroke(&self, pm: &mut Pixmap, path: &Path, c: [u8; 3], a: f32, width: f32) {
        if self.sees(path) {
            let mask = self.clip.as_ref().map(|c| &*c.mask);
            let s = Stroke {
                width: width / self.ts.sx,
                ..Stroke::default()
            };
            pm.stroke_path(path, &paint(c, a), &s, self.ts, mask);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broadcast::native::camera::{fit_box, map_aspect, MapBox};
    use crate::broadcast::native::draw::MAP_RECT;

    fn frames() -> (Frame, Frame) {
        let main = View::fit_home((0.0, 36.0, 900.0, 684.0));
        (Frame::main(main), Frame::inset(&main, &OKINAWA).unwrap())
    }

    #[test]
    fn the_inset_sits_at_the_top_left_and_does_not_reach_the_legend() {
        let (_, ins) = frames();
        let ((x, y, w, h), title) = ins.inset_box().unwrap();
        assert_eq!((x, y, h, title), (10.0, 46.0, 220.0, "南西諸島"));
        assert!((w - 225.9).abs() < 0.1, "{w}"); // 9 x cos(37) x 100 : 700 の縦横比
        assert!(y + h < 720.0 - 10.0 - 147.0 - 6.0 - 78.0); // 左下の凡例より上
    }

    #[test]
    fn the_inset_takes_only_points_in_its_range() {
        let (main, ins) = frames();
        assert!(ins.contains(127.68, 26.21)); // 那覇
        assert!(!ins.contains(139.69, 35.69)); // 東京
        assert!(main.contains(139.69, 35.69) && main.contains(127.68, 26.21));
        assert!(ins.is_inset() && !main.is_inset());
    }

    #[test]
    fn a_marker_just_outside_the_inset_is_pinned_to_its_edge() {
        let (main, ins) = frames();
        let ((x, y, w, h), _) = ins.inset_box().unwrap();
        // 台湾の東の海 (範囲の外だが 1.5 度以内) は、枠の左下の隅に寄る
        let (px, py) = ins.marker(121.5, 23.6).unwrap();
        assert_eq!((px, py), (x + 8.0, y + h - 8.0));
        // 枠の中はそのまま。遠い (東京) なら置かない。本図は常にそのまま
        let (nx, ny) = ins.marker(127.68, 26.21).unwrap();
        assert!(nx > x + 8.0 && nx < x + w - 8.0 && ny > y + 8.0 && ny < y + h - 8.0);
        assert_eq!(ins.marker(139.69, 35.69), None);
        assert_eq!(main.marker(139.69, 35.69), Some(main.view.px(139.69, 35.69)));
    }

    /// 千葉県北東部のあたりに寄った本図
    fn zoomed() -> (Frame, View) {
        let home = View::fit_home(MAP_RECT);
        let (x, y) = project(140.8, 35.7);
        let fit = fit_box(MapBox::around(x, y, 80.0).pad(), map_aspect());
        let view = View::from_fit(&fit, MAP_RECT);
        (Frame::zoomed(&home, view, &Frame::map_clip().unwrap()), home)
    }

    #[test]
    fn a_zoomed_main_maps_home_pixels_to_the_zoomed_view_and_centers_the_epicenter() {
        let (z, home) = zoomed();
        let (hx, hy) = home.px(140.8, 35.7);
        let (px, py) = z.point((hx, hy)).unwrap();
        let (vx, vy) = z.view.px(140.8, 35.7);
        assert!((px - vx).abs() < 0.01 && (py - vy).abs() < 0.01);
        // 地図の枠の中央 (x 450, y 36 + 342)
        assert!((px - 450.0).abs() < 1.0 && (py - 378.0).abs() < 1.0, "{px},{py}");
        assert!(!z.is_inset() && z.inset_box().is_none());
        // 日本全体の本図は、点をそのまま返す
        let main = Frame::main(home);
        assert_eq!(main.point((1000.0, -5.0)), Some((1000.0, -5.0)));
    }

    #[test]
    fn a_zoomed_main_skips_what_is_off_screen_and_does_not_place_marks_over_the_panel() {
        let (z, home) = zoomed();
        let rect = |x: f32, y: f32| {
            let mut b = PathBuilder::new();
            b.push_rect(Rect::from_xywh(x, y, 10.0, 10.0).unwrap());
            b.finish().unwrap()
        };
        // 札幌は日本全体の図では見えるが、寄った図では枠の外
        let (sx, sy) = home.px(141.35, 43.06);
        assert!(!z.sees(&rect(sx, sy)));
        let (cx, cy) = home.px(140.8, 35.7);
        assert!(z.sees(&rect(cx, cy)));
        assert!(Frame::main(home).sees(&rect(sx, sy)));
        assert_eq!(z.point((sx, sy)), None);
        assert!(z.marker(140.8, 35.7).is_some());
        assert_eq!(z.marker(141.35, 43.06), None); // 右パネルや上部バーの上には置かない
    }

    #[test]
    fn a_point_is_inside_a_path_in_screen_pixels_even_in_the_inset() {
        let (main, ins) = frames();
        let mut b = PathBuilder::new();
        b.push_rect(Rect::from_xywh(100.0, 100.0, 50.0, 40.0).unwrap());
        let path = b.finish().unwrap();
        assert!(main.path_contains(&path, (120.0, 120.0)) && !main.path_contains(&path, (160.0, 120.0)));
        assert_eq!(main.screen_bounds(&path), Some((100.0, 100.0, 50.0, 40.0)));
        // 別枠は path を縮めて映す: 本図の (100,100)-(150,140) は別枠では別の画面の位置になる
        let (x, y, w, h) = ins.screen_bounds(&path).unwrap();
        assert!(ins.path_contains(&path, (x + w / 2.0, y + h / 2.0)));
        assert!(!ins.path_contains(&path, (x + w + 1.0, y + h / 2.0)));
    }

    #[test]
    fn a_shape_far_from_the_inset_is_skipped() {
        let (_, ins) = frames();
        let mut far = PathBuilder::new();
        far.push_rect(Rect::from_xywh(600.0, 300.0, 50.0, 50.0).unwrap());
        assert!(!ins.sees(&far.finish().unwrap()));
    }
}
