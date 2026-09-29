// e2 での直接配信の検証: Rust で地図を描き (tiny-skia)、H.264 に圧縮する (openh264) 1 コマあたりの時間を測る。
// 使い方: e2spike <japan.geojson> <コマ数> [出力.h264]
use std::time::{Duration, Instant};

use openh264::encoder::{BitRate, Complexity, Encoder, EncoderConfig, FrameRate, UsageType};
use openh264::formats::{RgbaSliceU8, YUVBuffer};
use openh264::OpenH264API;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

const W: u32 = 1280;
const H: u32 = 720;

fn project(lon: f64, lat: f64) -> (f32, f32) {
    // 日本全体が地図の枠 (幅 900) に収まるようにする
    let x = (lon - 122.0) / (148.0 - 122.0) * 900.0;
    let y = (46.0 - lat) / (46.0 - 24.0) * 680.0 + 36.0;
    (x as f32, y as f32)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let geo: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&args[1]).unwrap()).unwrap();
    let frames: usize = args[2].parse().unwrap();
    let out = args.get(3);

    // 都道府県の輪郭を path にしておく (起動時に 1 回)
    let mut paths = Vec::new();
    for f in geo["features"].as_array().unwrap() {
        let mut pb = PathBuilder::new();
        for poly in f["geometry"]["coordinates"].as_array().unwrap() {
            for ring in poly.as_array().unwrap() {
                for (i, p) in ring.as_array().unwrap().iter().enumerate() {
                    let (x, y) = project(p[0].as_f64().unwrap(), p[1].as_f64().unwrap());
                    if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
                }
                pb.close();
            }
        }
        if let Some(p) = pb.finish() {
            paths.push(p);
        }
    }

    let cfg = EncoderConfig::new()
        .bitrate(BitRate::from_bps(1_000_000))
        .max_frame_rate(FrameRate::from_hz(10.0))
        .usage_type(UsageType::ScreenContentRealTime)
        .complexity(Complexity::Low);
    let mut enc = Encoder::with_api_config(OpenH264API::from_source(), cfg).unwrap();
    let mut pixmap = Pixmap::new(W, H).unwrap();
    let mut file = out.map(|p| std::fs::File::create(p).unwrap());
    let (mut t_draw, mut t_yuv, mut t_enc, mut bytes) = (Duration::ZERO, Duration::ZERO, Duration::ZERO, 0usize);

    for n in 0..frames {
        let t0 = Instant::now();
        // 毎コマ描き直す (地図・点・時計の枠)。実際の画面より文字が無い分だけ軽い
        pixmap.fill(Color::from_rgba8(13, 17, 23, 255));
        let mut land = Paint::default();
        land.set_color_rgba8(58, 63, 75, 255);
        land.anti_alias = true;
        let mut edge = Paint::default();
        edge.set_color_rgba8(90, 96, 110, 255);
        edge.anti_alias = true;
        let stroke = Stroke { width: 0.8, ..Stroke::default() };
        for p in &paths {
            pixmap.fill_path(p, &land, FillRule::EvenOdd, Transform::identity(), None);
            pixmap.stroke_path(p, &edge, &stroke, Transform::identity(), None);
        }
        let mut dot = Paint::default();
        dot.set_color_rgba8(160, 210, 255, 255);
        dot.anti_alias = true;
        for i in 0..300 {
            let (x, y) = (300.0 + (i * 37 % 500) as f32, 200.0 + (i * 53 % 400) as f32);
            if let Some(c) = PathBuilder::from_circle(x, y, 3.0) {
                pixmap.fill_path(&c, &dot, FillRule::Winding, Transform::identity(), None);
            }
        }
        let mut clock = Paint::default();
        clock.set_color_rgba8(30, (n * 7 % 255) as u8, 60, 255);
        pixmap.fill_rect(tiny_skia::Rect::from_xywh(960.0, 620.0, 300.0, 90.0).unwrap(), &clock, Transform::identity(), None);
        let t1 = Instant::now();
        let yuv = YUVBuffer::from_rgb_source(RgbaSliceU8::new(pixmap.data(), (W as usize, H as usize)));
        let t2 = Instant::now();
        let bs = enc.encode(&yuv).unwrap();
        let v = bs.to_vec();
        let t3 = Instant::now();
        bytes += v.len();
        if let Some(f) = file.as_mut() {
            use std::io::Write;
            f.write_all(&v).unwrap();
        }
        t_draw += t1 - t0;
        t_yuv += t2 - t1;
        t_enc += t3 - t2;
    }
    let per = |d: Duration| d.as_secs_f64() * 1000.0 / frames as f64;
    let total = per(t_draw) + per(t_yuv) + per(t_enc);
    println!(
        "frames={frames} per frame: draw={:.1}ms yuv={:.1}ms encode={:.1}ms total={:.1}ms | 1 core: {:.0}% at 5fps, {:.0}% at 10fps, {:.0}% at 30fps | {:.2} Mbps at 10fps",
        per(t_draw), per(t_yuv), per(t_enc), total, total * 5.0 / 10.0, total * 10.0 / 10.0, total * 30.0 / 10.0,
        bytes as f64 * 8.0 / frames as f64 * 10.0 / 1e6
    );
}
