//! 図形を描く小さな道具 (色は [r, g, b] と不透明度)。

#![allow(clippy::too_many_arguments)]

use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

// web/public/style.css の :root と同じ色
pub const BG: [u8; 3] = [0x0d, 0x11, 0x17];
pub const PANEL: [u8; 3] = [0x15, 0x1b, 0x23];
pub const LINE: [u8; 3] = [0x2a, 0x32, 0x3d];
pub const TEXT: [u8; 3] = [0xe6, 0xed, 0xf3];
pub const MUTED: [u8; 3] = [0x8b, 0x94, 0x9e];
pub const LAND: [u8; 3] = [0x3a, 0x42, 0x50];
pub const LAND_EDGE: [u8; 3] = [0x0d, 0x11, 0x17];
pub const SEA: [u8; 3] = [0x0a, 0x0f, 0x16];

pub fn paint(c: [u8; 3], a: f32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c[0], c[1], c[2], (a.clamp(0.0, 1.0) * 255.0).round() as u8);
    p.anti_alias = true;
    p
}

pub fn rect(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, c: [u8; 3], a: f32) {
    if let Some(r) = Rect::from_xywh(x, y, w, h) {
        let mut p = paint(c, a);
        p.anti_alias = false;
        pm.fill_rect(r, &p, Transform::identity(), None);
    }
}

/// 角の丸い四角 (半径 r)
pub fn rrect(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, c: [u8; 3], a: f32) {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    if let Some(path) = pb.finish() {
        pm.fill_path(&path, &paint(c, a), FillRule::Winding, Transform::identity(), None);
    }
}

pub fn circle(pm: &mut Pixmap, x: f32, y: f32, r: f32, c: [u8; 3], a: f32) {
    if let Some(path) = PathBuilder::from_circle(x, y, r) {
        pm.fill_path(&path, &paint(c, a), FillRule::Winding, Transform::identity(), None);
    }
}

/// 線 (太さ w)
pub fn line(pm: &mut Pixmap, from: (f32, f32), to: (f32, f32), w: f32, c: [u8; 3], a: f32) {
    let mut pb = PathBuilder::new();
    pb.move_to(from.0, from.1);
    pb.line_to(to.0, to.1);
    if let Some(path) = pb.finish() {
        let s = Stroke {
            width: w,
            ..Stroke::default()
        };
        pm.stroke_path(&path, &paint(c, a), &s, Transform::identity(), None);
    }
}

/// 白い縁取り付きの × (震央)
pub fn epicenter(pm: &mut Pixmap, x: f32, y: f32) {
    let d = 9.0;
    for (w, c) in [(7.0, [255, 255, 255]), (3.5, [0xe0, 0x1e, 0x1e])] {
        line(pm, (x - d, y - d), (x + d, y + d), w, c, 1.0);
        line(pm, (x - d, y + d), (x + d, y - d), w, c, 1.0);
    }
}
