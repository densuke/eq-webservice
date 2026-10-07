//! 地図の投影と、GeoJSON (MultiPolygon) を画面の座標の path にする。投影は web/src/map.ts と同じ。

use anyhow::Context;
use serde::Deserialize;
use std::collections::HashMap;
use tiny_skia::{Path, PathBuilder, Transform};

use super::camera::{Fit, MapBox};

const LON0: f64 = 137.0;
const LAT0: f64 = 37.0;
const KY: f64 = 100.0;

/// 日本全体が収まる表示範囲 (web/src/map.ts の HOME)
const HOME: (f64, f64, f64, f64) = (128.0, 146.2, 30.0, 45.8);

/// 日本全体の表示範囲 (地図の座標 x0, y0, x1, y1)
pub fn home_bounds() -> (f64, f64, f64, f64) {
    let (x0, y0) = project(HOME.0, HOME.3);
    let (x1, y1) = project(HOME.1, HOME.2);
    (x0, y0, x1, y1)
}

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

    /// 地図の座標の表示範囲 fit (縦横比は rect に合っている) が rect いっぱいに映るようにする
    pub fn from_fit(fit: &Fit, rect: (f64, f64, f64, f64)) -> View {
        let scale = rect.2 / fit.w;
        View {
            scale,
            ox: rect.0 - fit.x * scale,
            oy: rect.1 - fit.y * scale,
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

/// 緯度 1 度の長さ (km)。地図の座標の縦 (KY) がこれに当たる
const KM_PER_DEG_LAT: f64 = 111.19;

impl View {
    /// 震央から半径 radius_km の円 (画面の座標の path)。投影は緯度で横に伸びるので、その分を直した楕円で近似する
    pub fn circle(&self, lat: f64, lon: f64, radius_km: f64) -> Option<Path> {
        let (cx, cy) = self.px(lon, lat);
        let ry = radius_km / KM_PER_DEG_LAT * KY * self.scale;
        let rx = ry * (LAT0.to_radians().cos() / lat.to_radians().cos());
        let (rx, ry) = (rx as f32, ry as f32);
        PathBuilder::from_oval(tiny_skia::Rect::from_xywh(cx - rx, cy - ry, rx * 2.0, ry * 2.0)?)
    }
}

/// 1 つの区域 (都道府県・市町村等)
pub struct Shape {
    /// properties の name (都道府県) か code (市町村等)
    pub key: String,
    pub path: Path,
    /// 印を置く場所 (いちばん大きい島の外接矩形の中心)
    pub center: (f32, f32),
    /// 外接矩形 (地図の座標。寄りに使う)。bounds は離島も含み、main は一番大きい島だけ
    pub bounds: MapBox,
    pub main: MapBox,
}

/// 点列の外接矩形 (地図の座標)
fn box_of(pts: &[[f64; 2]]) -> Option<MapBox> {
    pts.iter()
        .map(|p| project(p[0], p[1]))
        .map(|(x, y)| MapBox {
            x0: x,
            y0: y,
            x1: x,
            y1: y,
        })
        .reduce(|a, b| MapBox::union(Some(a), Some(b)).unwrap_or(a))
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
        let (mut bounds, mut main): (Option<MapBox>, Option<(f64, MapBox)>) = (None, None);
        for poly in &f.geometry.coordinates {
            for (ri, ring) in poly.iter().enumerate() {
                let pts: Vec<(f32, f32)> = ring.iter().map(|p| view.px(p[0], p[1])).collect();
                let Some(&(fx, fy)) = pts.first() else { continue };
                pb.move_to(fx, fy);
                for &(x, y) in &pts[1..] {
                    pb.line_to(x, y);
                }
                pb.close();
                let ring_box = box_of(ring);
                bounds = MapBox::union(bounds, ring_box);
                if let (0, Some(b)) = (ri, ring_box) {
                    let area = (b.x1 - b.x0) * (b.y1 - b.y0);
                    if main.is_none_or(|(a, _)| area >= a) {
                        main = Some((area, b));
                    }
                }
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
        if let (Some(path), Some(bounds), Some((_, main))) = (pb.finish(), bounds, main) {
            out.push(Shape {
                key: name,
                path,
                center: biggest.1,
                bounds,
                main,
            });
        }
    }
    Ok(out)
}

/// 津波予報区の海岸線 (GeoJSON の MultiLineString)。name と、開いた線の path (本図の座標)
pub struct Coast {
    pub name: String,
    pub path: Path,
}

#[derive(Deserialize)]
struct LineDoc {
    features: Vec<LineFeature>,
}

#[derive(Deserialize)]
struct LineFeature {
    properties: HashMap<String, serde_json::Value>,
    geometry: LineGeometry,
}

#[derive(Deserialize)]
struct LineGeometry {
    coordinates: Vec<Vec<[f64; 2]>>,
}

pub fn parse_coast(json: &str, view: &View) -> anyhow::Result<Vec<Coast>> {
    let doc: LineDoc = serde_json::from_str(json).context("parsing coast geojson")?;
    let mut out = Vec::with_capacity(doc.features.len());
    for f in doc.features {
        let mut pb = PathBuilder::new();
        for line in &f.geometry.coordinates {
            let mut pts = line.iter().map(|p| view.px(p[0], p[1]));
            let Some((x, y)) = pts.next() else { continue };
            pb.move_to(x, y);
            for (x, y) in pts {
                pb.line_to(x, y);
            }
        }
        let name = f.properties.get("name").and_then(|v| v.as_str()).unwrap_or_default();
        if let Some(path) = pb.finish() {
            out.push(Coast {
                name: name.to_string(),
                path,
            });
        }
    }
    Ok(out)
}

pub fn load_coast(file: &std::path::Path, view: &View) -> anyhow::Result<Vec<Coast>> {
    let text = std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    parse_coast(&text, view).with_context(|| file.display().to_string())
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
    fn a_circle_is_round_at_the_origin_latitude_and_wider_in_the_north() {
        let view = View::fit_home((0.0, 36.0, 900.0, 684.0));
        let km = |lat: f64, r: f64| {
            let b = view.circle(lat, 137.0, r).unwrap().bounds();
            (b.width(), b.height())
        };
        let (w, h) = km(37.0, 111.19); // 緯度 1 度分の半径
        assert!((w - h).abs() < 0.01 && w > 0.0, "{w} {h}");
        // 緯度 1 度 = 地図の縦 100 なので、直径は 200 x scale
        let (_, h1) = km(37.0, 55.595);
        assert!((h - 2.0 * h1).abs() < 0.01);
        let (wn, hn) = km(60.0, 111.19);
        assert!((hn - h).abs() < 0.01); // 縦は同じ長さ
        assert!(wn > w * 1.5, "{wn}"); // 北ほど経度が詰まるので、横に伸びる (cos 37 / cos 60 = 1.6)
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
        let (w, h) = ((131.5f64 - 122.5) * 37f64.to_radians().cos() * 100.0, 700.0);
        let rect = (10.0, 46.0, 220.0 * w / h, 220.0);
        let inset = View::fit((122.5, 131.5, 24.0, 31.0), rect);
        // 枠の四隅が枠に収まる (縦横比が合っているので、ちょうど埋まる)
        let (l, t) = inset.px(122.5, 31.0);
        let (r, b) = inset.px(131.5, 24.0);
        assert!((l - 10.0).abs() < 0.01 && (t - 46.0).abs() < 0.01, "{l},{t}");
        assert!(
            (r as f64 - rect.0 - rect.2).abs() < 0.01 && (b - 266.0).abs() < 0.01,
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
        // 寄りに使う外接矩形 (地図の座標): bounds は離島も含み、main は一番大きい島だけ
        let (x0, y0) = project(130.0, 37.0);
        let (x1, y1) = project(138.0, 30.0);
        let b = shapes[0].bounds;
        assert_eq!((b.x0, b.y0, b.x1, b.y1), (x0, y0, x1, y1));
        let (mx0, my0) = project(137.0, 37.0);
        let (mx1, my1) = project(138.0, 36.0);
        let m = shapes[0].main;
        assert_eq!((m.x0, m.y0, m.x1, m.y1), (mx0, my0, mx1, my1));
        assert!(parse("{}", "name", &v).is_err());
    }
}
