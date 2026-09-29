//! 地図を映す面 (本図と、離島の別枠)。path は本図の座標で 1 度だけ作り、別枠は変換をかけて映す
//! (別枠は枠の外に描かない)。web/src/map.ts の INSETS と同じ。

use tiny_skia::{FillRule, Mask, Path, PathBuilder, Pixmap, Rect, Stroke, Transform};

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
    lon: (122.9, 131.4),
    lat: (24.0, 30.0),
    x: 10.0,
    y: super::draw::BAR_H + 10.0,
    h: 150.0,
};

/// 枠 (x, y, 幅, 高さ)
pub type BoxRect = (f32, f32, f32, f32);

pub struct Frame {
    /// この面の経度・緯度 -> 画面の座標
    pub view: View,
    /// 本図の path をこの面に映す変換 (本図は何もしない)
    ts: Transform,
    /// 別枠だけ: (枠の外を隠す型、枠、範囲 lon0 lon1 lat0 lat1、題)
    inset: Option<Inset>,
}

struct Inset {
    mask: Mask,
    rect: Rect,
    bounds: (f64, f64, f64, f64),
    title: &'static str,
}

impl Frame {
    pub fn main(view: View) -> Frame {
        Frame {
            view,
            ts: Transform::identity(),
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
        let mut mask = Mask::new(super::draw::W, super::draw::H)?;
        mask.fill_path(
            &PathBuilder::from_rect(rect),
            FillRule::Winding,
            false,
            Transform::identity(),
        );
        Some(Frame {
            ts: view.transform_from(main),
            view,
            inset: Some(Inset {
                mask,
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
        self.inset
            .as_ref()
            .is_none_or(|i| (i.bounds.0..=i.bounds.1).contains(&lon) && (i.bounds.2..=i.bounds.3).contains(&lat))
    }

    /// この path が面の中に見えるか (見えないものは描かずに済ませる)
    pub fn sees(&self, path: &Path) -> bool {
        let Some(i) = &self.inset else { return true };
        path.bounds().transform(self.ts).is_some_and(|b| {
            b.left() < i.rect.right()
                && b.right() > i.rect.left()
                && b.top() < i.rect.bottom()
                && b.bottom() > i.rect.top()
        })
    }

    pub fn fill(&self, pm: &mut Pixmap, path: &Path, c: [u8; 3], a: f32) {
        if self.sees(path) {
            let mask = self.inset.as_ref().map(|i| &i.mask);
            pm.fill_path(path, &paint(c, a), FillRule::EvenOdd, self.ts, mask);
        }
    }

    /// 線の太さ width は画面での太さ (別枠でも同じ太さに見える)
    pub fn stroke(&self, pm: &mut Pixmap, path: &Path, c: [u8; 3], a: f32, width: f32) {
        if self.sees(path) {
            let mask = self.inset.as_ref().map(|i| &i.mask);
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

    fn frames() -> (Frame, Frame) {
        let main = View::fit_home((0.0, 36.0, 900.0, 684.0));
        (Frame::main(main), Frame::inset(&main, &OKINAWA).unwrap())
    }

    #[test]
    fn the_inset_sits_at_the_top_left_and_does_not_reach_the_legend() {
        let (_, ins) = frames();
        let ((x, y, w, h), title) = ins.inset_box().unwrap();
        assert_eq!((x, y, h, title), (10.0, 46.0, 150.0, "南西諸島"));
        assert!((w - 169.8).abs() < 0.1, "{w}"); // 8.5 x cos(37) x 100 : 600 の縦横比
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
    fn a_shape_far_from_the_inset_is_skipped() {
        let (_, ins) = frames();
        let mut far = PathBuilder::new();
        far.push_rect(Rect::from_xywh(600.0, 300.0, 50.0, 50.0).unwrap());
        assert!(!ins.sees(&far.finish().unwrap()));
    }
}
