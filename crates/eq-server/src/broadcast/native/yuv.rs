//! RGBA -> I420 (YUV 4:2:0 の平面: Y、U、V の順)。BT.601 の限定範囲 (Y 16..235、U・V 16..240) で、
//! ffmpeg の既定の変換 (swscale) と同じ。描き直したときだけ呼び、ffmpeg に色の変換をさせない。
//! 画面は不透明なので、alpha は見ない。U・V は 2x2 の平均から求める。

/// 1 画素の Y
fn luma(r: i32, g: i32, b: i32) -> u8 {
    (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16) as u8
}

/// 平均した色の U・V
fn chroma(r: i32, g: i32, b: i32) -> (u8, u8) {
    let u = ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128;
    let v = ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128;
    (u.clamp(16, 240) as u8, v.clamp(16, 240) as u8)
}

/// w x h の RGBA (4 バイト/画素) を I420 (w*h*3/2 バイト) にする。w・h は偶数
pub fn rgba_to_i420(rgba: &[u8], w: usize, h: usize) -> Vec<u8> {
    assert!(
        w.is_multiple_of(2) && h.is_multiple_of(2) && rgba.len() == w * h * 4,
        "bad frame size"
    );
    let (mut y_plane, mut u_plane, mut v_plane) = (
        Vec::with_capacity(w * h),
        Vec::with_capacity(w * h / 4),
        Vec::with_capacity(w * h / 4),
    );
    let px = |x: usize, y: usize| {
        let i = (y * w + x) * 4;
        (rgba[i] as i32, rgba[i + 1] as i32, rgba[i + 2] as i32)
    };
    for y in 0..h {
        for x in 0..w {
            let (r, g, b) = px(x, y);
            y_plane.push(luma(r, g, b));
        }
    }
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            let block = [px(x, y), px(x + 1, y), px(x, y + 1), px(x + 1, y + 1)];
            let sum = |f: fn(&(i32, i32, i32)) -> i32| (block.iter().map(f).sum::<i32>() + 2) / 4;
            let (u, v) = chroma(sum(|c| c.0), sum(|c| c.1), sum(|c| c.2));
            u_plane.push(u);
            v_plane.push(v);
        }
    }
    y_plane.extend(u_plane);
    y_plane.extend(v_plane);
    y_plane
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 単色の 2x2 を変換した (Y, U, V)
    fn solid(rgb: [u8; 3]) -> (u8, u8, u8) {
        let rgba: Vec<u8> = (0..4).flat_map(|_| [rgb[0], rgb[1], rgb[2], 255]).collect();
        let out = rgba_to_i420(&rgba, 2, 2);
        assert_eq!(out.len(), 6); // Y 4 + U 1 + V 1
        assert!(out[..4].iter().all(|&y| y == out[0]));
        (out[0], out[4], out[5])
    }

    #[test]
    fn known_colors_match_bt601_limited_range() {
        assert_eq!(solid([255, 255, 255]), (235, 128, 128)); // 白
        assert_eq!(solid([0, 0, 0]), (16, 128, 128)); // 黒
        assert_eq!(solid([255, 0, 0]), (82, 90, 240)); // 赤
        assert_eq!(solid([0x0a, 0x0f, 0x16]), (28, 132, 125)); // 海の色
    }

    #[test]
    fn planes_are_y_then_u_then_v_and_chroma_is_averaged() {
        // 4x2: 左の 2x2 は白、右の 2x2 は赤
        let px = |x: usize| if x < 2 { [255, 255, 255, 255] } else { [255, 0, 0, 255] };
        let rgba: Vec<u8> = (0..2).flat_map(|_| (0..4).flat_map(px)).collect();
        let out = rgba_to_i420(&rgba, 4, 2);
        assert_eq!(out.len(), 12);
        assert_eq!(&out[..4], &[235, 235, 82, 82]);
        assert_eq!(&out[4..8], &[235, 235, 82, 82]);
        assert_eq!(&out[8..10], &[128, 90]); // U
        assert_eq!(&out[10..12], &[128, 240]); // V
                                               // 白と黒が半分ずつなら、色差は中立のまま
        let mixed = [255, 255, 255, 255, 0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255];
        assert_eq!(&rgba_to_i420(&mixed, 2, 2)[4..], &[128, 128]);
    }
}
