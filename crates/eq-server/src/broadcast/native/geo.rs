//! 地図の投影と、GeoJSON (MultiPolygon) を画面の座標の path にする。投影は web/src/map.ts と同じ。

use anyhow::Context;
use serde::Deserialize;
use std::collections::HashMap;
use tiny_skia::{Path, PathBuilder, Transform};

const LON0: f64 = 137.0;
const LAT0: f64 = 37.0;
const KY: f64 = 100.0;

/// 日本全体が収まる表示範囲 (web/src/map.ts の HOME)
const HOME: (f64, f64, f64, f64) = (128.0, 146.2, 30.0, 45.8);

/// 地図の座標 (経度・緯度 → x, y。137°E 37°N が原点、北が上)
pub fn project(lon: f64, lat: f64) -> (f64, f64) {
    let kx = (LAT0.to_radians()).cos() * 100.0;
    ((lon - LON0) * kx, -(lat - LAT0) * KY)
}

/// 地図の座標から画面の座標への拡大・移動
#[derive(Debug, Clone, Copy)]
pub struct View {
    scale: f64,
    ox: f64,
    oy: f64,
}

impl View {
    /// 日本全体が、縦横比を保って rect (x, y, w, h) の中央に収まるようにする
    pub fn fit_home(rect: (f64, f64, f64, f64)) -> View {
        View::fit((HOME.0, HOME.1, HOME.2, HOME.3), rect)
    }

    /// 経度 lon0..lon1・緯度 lat0..lat1 が、縦横比を保って rect (x, y, w, h) の中央に収まるようにする
    pub fn fit((lon0, lon1, lat0, lat1): (f64, f64, f64, f64), rect: (f64, f64, f64, f64)) -> View {
        let (x0, y0) = project(lon0, lat1);
        let (x1, y1) = project(lon1, lat0);
        let (rx, ry, rw, rh) = rect;
        let scale = (rw / (x1 - x0)).min(rh / (y1 - y0));
        View {
            scale,
            ox: rx + (rw - (x1 - x0) * scale) / 2.0 - x0 * scale,
            oy: ry + (rh - (y1 - y0) * scale) / 2.0 - y0 * scale,
        }
    }

    /// main の画面の座標を、この表示 (別枠) の画面の座標に移す変換 (main で作った path を別枠に映すのに使う)
    pub fn transform_from(&self, main: &View) -> Transform {
        let k = (self.scale / main.scale) as f32;
        Transform::from_row(
            k,
            0.0,
            0.0,
            k,
            (self.ox - main.ox * k as f64) as f32,
            (self.oy - main.oy * k as f64) as f32,
        )
    }

    /// 経度・緯度の画面の座標
    pub fn px(&self, lon: f64, lat: f64) -> (f32, f32) {
        let (x, y) = project(lon, lat);
        ((x * self.scale + self.ox) as f32, (y * self.scale + self.oy) as f32)
    }
}

/// 1 つの区域 (都道府県・市町村等)
pub struct Shape {
    /// properties の name (都道府県) か code (市町村等)
    pub key: String,
    pub path: Path,
    /// 印を置く場所 (いちばん大きい島の外接矩形の中心)
    pub center: (f32, f32),
}

#[derive(Deserialize)]
struct Collection {
    features: Vec<Feature>,
}

#[derive(Deserialize)]
struct Feature {
    #[serde(default)]
    properties: HashMap<String, serde_json::Value>,
    geometry: Geometry,
}

/// MultiPolygon の座標 (多角形 → 輪 → 点)
#[derive(Deserialize)]
struct Geometry {
    coordinates: Vec<Vec<Vec<[f64; 2]>>>,
}

/// GeoJSON の文字列から区域を作る。key は properties の項目名。path にしたら座標は捨てる
pub fn parse(json: &str, key: &str, view: &View) -> anyhow::Result<Vec<Shape>> {
    let doc: Collection = serde_json::from_str(json).context("parsing geojson")?;
    let mut out = Vec::with_capacity(doc.features.len());
    for f in doc.features {
        let name = f
            .properties
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let mut pb = PathBuilder::new();
        let mut biggest = (0.0f32, (0.0f32, 0.0f32));
        for poly in &f.geometry.coordinates {
            for (ri, ring) in poly.iter().enumerate() {
                let pts: Vec<(f32, f32)> = ring.iter().map(|p| view.px(p[0], p[1])).collect();
                let Some(&(fx, fy)) = pts.first() else { continue };
                pb.move_to(fx, fy);
                for &(x, y) in &pts[1..] {
                    pb.line_to(x, y);
                }
                pb.close();
                if ri == 0 {
                    let (mut lo, mut hi) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
                    for &(x, y) in &pts {
                        lo = (lo.0.min(x), lo.1.min(y));
                        hi = (hi.0.max(x), hi.1.max(y));
                    }
                    let area = (hi.0 - lo.0) * (hi.1 - lo.1);
                    if area >= biggest.0 {
                        biggest = (area, ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0));
                    }
                }
            }
        }
        if let Some(path) = pb.finish() {
            out.push(Shape {
                key: name,
                path,
                center: biggest.1,
            });
        }
    }
    Ok(out)
}

pub fn load(file: &std::path::Path, key: &str, view: &View) -> anyhow::Result<Vec<Shape>> {
    let text = std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    parse(&text, key, view).with_context(|| file.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_matches_the_page() {
        assert_eq!(project(137.0, 37.0), (0.0, 0.0));
        let (x, y) = project(139.0, 36.0);
        assert!((x - 2.0 * 37f64.to_radians().cos() * 100.0).abs() < 1e-9);
        assert!((y - 100.0).abs() < 1e-9); // 南へ行くほど y が増える
    }

    #[test]
    fn japan_fits_in_the_map_rect() {
        let rect = (0.0, 36.0, 900.0, 684.0);
        let v = View::fit_home(rect);
        for (lon, lat) in [(128.0, 45.8), (146.2, 30.0), (137.0, 37.0)] {
            let (x, y) = v.px(lon, lat);
            assert!(
                (0.0..=900.0).contains(&x) && (36.0..=720.0).contains(&y),
                "{lon},{lat} -> {x},{y}"
            );
        }
        // 縦が足りないので縦いっぱいに拡大し、左右は余る
        let (_, top) = v.px(137.0, 45.8);
        let (_, bottom) = v.px(137.0, 30.0);
        assert!((top - 36.0).abs() < 0.01 && (bottom - 720.0).abs() < 0.01);
    }

    #[test]
    fn an_inset_view_maps_points_like_the_main_view_through_the_transform() {
        let main = View::fit_home((0.0, 36.0, 900.0, 684.0));
        let (w, h) = ((131.4f64 - 122.9) * 37f64.to_radians().cos() * 100.0, 600.0);
        let rect = (10.0, 46.0, 150.0 * w / h, 150.0);
        let inset = View::fit((122.9, 131.4, 24.0, 30.0), rect);
        // 枠の四隅が枠に収まる (縦横比が合っているので、ちょうど埋まる)
        let (l, t) = inset.px(122.9, 30.0);
        let (r, b) = inset.px(131.4, 24.0);
        assert!((l - 10.0).abs() < 0.01 && (t - 46.0).abs() < 0.01, "{l},{t}");
        assert!(
            (r as f64 - rect.0 - rect.2).abs() < 0.01 && (b - 196.0).abs() < 0.01,
            "{r},{b}"
        );
        // 那覇: 本図の座標を変換で映した位置 = 別枠の投影
        let ts = inset.transform_from(&main);
        let (mx, my) = main.px(127.68, 26.21);
        let (ix, iy) = inset.px(127.68, 26.21);
        assert!((ts.sx * mx + ts.tx - ix).abs() < 0.01 && (ts.sy * my + ts.ty - iy).abs() < 0.01);
    }

    #[test]
    fn builds_paths_and_marks_the_biggest_island() {
        let json = r#"{"features":[{"properties":{"name":"A","code":"1"},"geometry":{"type":"MultiPolygon","coordinates":[
            [[[137,37],[138,37],[138,36],[137,36],[137,37]]],
            [[[130,30],[130.1,30],[130.1,30.1],[130,30.1],[130,30]]]]}}]}"#;
        let v = View::fit_home((0.0, 36.0, 900.0, 684.0));
        let shapes = parse(json, "name", &v).unwrap();
        assert_eq!(shapes.len(), 1);
        assert_eq!(shapes[0].key, "A");
        let (cx, cy) = v.px(137.5, 36.5);
        assert!((shapes[0].center.0 - cx).abs() < 0.01 && (shapes[0].center.1 - cy).abs() < 0.01);
        assert!(parse("{}", "name", &v).is_err());
    }
}
