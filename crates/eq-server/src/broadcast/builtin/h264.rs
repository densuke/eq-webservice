//! openh264 で I420 の画面を H.264 (Annex B) に圧縮する。

use anyhow::Context;
use openh264::encoder::{BitRate, Complexity, Encoder, EncoderConfig, FrameRate, RateControlMode, UsageType};
use openh264::formats::YUVSlices;
use openh264::{OpenH264API, Timestamp};

use super::flv;

/// キーフレームの間隔 (YouTube などは 4 秒以下を求める)
const KEY_EVERY_MS: u64 = 2000;

pub struct H264 {
    enc: Encoder,
    width: usize,
    height: usize,
    last_key_ms: Option<u64>,
}

/// 圧縮した 1 コマ
pub struct Encoded {
    /// Annex B のまま (SPS・PPS・SEI・スライス)
    pub annex_b: Vec<u8>,
    pub keyframe: bool,
}

impl H264 {
    pub fn new(width: u32, height: u32, bitrate_bps: u32, max_fps: u32) -> anyhow::Result<Self> {
        anyhow::ensure!(
            width.is_multiple_of(2) && height.is_multiple_of(2),
            "width・height は偶数にしてください"
        );
        let cfg = EncoderConfig::new()
            .usage_type(UsageType::ScreenContentRealTime)
            .rate_control_mode(RateControlMode::Bitrate)
            .bitrate(BitRate::from_bps(bitrate_bps))
            .max_frame_rate(FrameRate::from_hz(max_fps.max(1) as f32))
            .complexity(Complexity::Low)
            // 目標ビットレートを守るには、超えたコマを飛ばす必要がある (飛ばしたコマは何も出力されず、時刻はそのまま進む)
            .skip_frames(true);
        let enc = Encoder::with_api_config(OpenH264API::from_source(), cfg).context("openh264 の初期化")?;
        Ok(Self {
            enc,
            width: width as usize,
            height: height as usize,
            last_key_ms: None,
        })
    }

    /// 1 コマ圧縮する。前のキーフレームから 2000ms 以上たっていたら、キーフレームにする
    pub fn encode(&mut self, i420: &[u8], pts_ms: u64) -> anyhow::Result<Encoded> {
        let (w, h) = (self.width, self.height);
        anyhow::ensure!(i420.len() == w * h * 3 / 2, "画面の大きさが違います");
        if need_key(self.last_key_ms, pts_ms) {
            self.enc.force_intra_frame();
        }
        let (y, uv) = i420.split_at(w * h);
        let (u, v) = uv.split_at(w * h / 4);
        let yuv = YUVSlices::new((y, u, v), (w, h), (w, w / 2, w / 2));
        let out = self
            .enc
            .encode_at(&yuv, Timestamp::from_millis(pts_ms))
            .context("H.264 の圧縮")?;
        let mut annex_b = Vec::new();
        out.write_vec(&mut annex_b);
        let keyframe = flv::split_annex_b(&annex_b).iter().any(|n| flv::nal_type(n) == 5);
        if keyframe {
            self.last_key_ms = Some(pts_ms);
        }
        Ok(Encoded { annex_b, keyframe })
    }
}

/// キーフレームを強制するか (最初のコマ、または前のキーフレームから 2000ms 以上)
fn need_key(last_key_ms: Option<u64>, pts_ms: u64) -> bool {
    last_key_ms.is_none_or(|k| pts_ms.saturating_sub(k) >= KEY_EVERY_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray(w: usize, h: usize) -> Vec<u8> {
        let mut v = vec![100u8; w * h];
        v.extend(vec![128u8; w * h / 2]);
        v
    }

    #[test]
    fn key_frames_are_forced_by_time() {
        assert!(need_key(None, 0));
        assert!(!need_key(Some(0), 1999));
        assert!(need_key(Some(0), 2000));
        // 間隔が一定でなくても (可変 fps)、時刻で決める
        assert!(!need_key(Some(2100), 4000));
        assert!(need_key(Some(2100), 4100));
    }

    #[test]
    fn a_flat_screen_gives_a_keyframe_with_sps_and_pps_then_key_every_2s() {
        let (w, h) = (320, 180);
        let mut e = H264::new(w as u32, h as u32, 300_000, 10).unwrap();
        let img = gray(w, h);
        let first = e.encode(&img, 0).unwrap();
        assert!(first.keyframe);
        let types: Vec<u8> = flv::split_annex_b(&first.annex_b)
            .iter()
            .map(|n| flv::nal_type(n))
            .collect();
        assert!(
            types.contains(&7) && types.contains(&8) && types.contains(&5),
            "{types:?}"
        );
        let keys: Vec<u64> = [500u64, 1000, 1500, 2000, 2500, 3900, 4000]
            .into_iter()
            .filter(|&t| e.encode(&img, t).unwrap().keyframe)
            .collect();
        assert_eq!(keys, [2000, 4000]);
    }

    #[test]
    fn a_wrong_sized_screen_is_an_error() {
        let mut e = H264::new(320, 180, 300_000, 10).unwrap();
        assert!(e.encode(&[0; 10], 0).is_err());
    }
}
