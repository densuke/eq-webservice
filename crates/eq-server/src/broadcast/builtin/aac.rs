//! 無音の AAC-LC (44.1kHz・ステレオ)。1 コマ (1024 サンプル) の決まったバイト列を、映像の時刻に合わせて並べる。

/// AudioSpecificConfig: AAC-LC (2)、44100Hz (index 4)、ステレオ (2)
pub const AUDIO_SPECIFIC_CONFIG: [u8; 2] = [0x12, 0x10];

/// 無音 1 コマの生データ (ADTS のヘッダを除いたもの)。
/// 作り方: `ffmpeg -f lavfi -i anullsrc=r=44100:cl=stereo -t 0.5 -c:a aac -b:a 32k -f adts out.aac` の
/// 2 コマ目以降 (最初の 1 コマだけ 21 バイトで、エンコーダの名前が入る) を取り出す
pub const SILENT_FRAME: [u8; 6] = [0x21, 0x10, 0x04, 0x60, 0x8c, 0x1c];

const SAMPLES_PER_FRAME: u64 = 1024;
const SAMPLE_RATE: u64 = 44100;

/// n 番目のコマの時刻 (ミリ秒)。1024 / 44100 秒 = 約 23.2ms ごと
pub fn frame_ms(n: u64) -> u64 {
    n * SAMPLES_PER_FRAME * 1000 / SAMPLE_RATE
}

/// 音のコマの番号を数え、映像の時刻までに出すべきコマの時刻を返す
#[derive(Debug, Default)]
pub struct Silence {
    next: u64,
}

impl Silence {
    /// pts_ms 以前の時刻の音のコマ (時刻のミリ秒) を、まだ出していない分だけ返す
    pub fn until(&mut self, pts_ms: u64) -> Vec<u64> {
        let mut out = Vec::new();
        while frame_ms(self.next) <= pts_ms {
            out.push(frame_ms(self.next));
            self.next += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_23_2ms_apart() {
        assert_eq!(frame_ms(0), 0);
        assert_eq!(frame_ms(1), 23);
        assert_eq!(frame_ms(2), 46);
        // 1 秒で 43 コマ (44100 / 1024 = 43.07)
        assert_eq!(frame_ms(43), 998);
        assert_eq!(frame_ms(44), 1021);
    }

    #[test]
    fn audio_is_emitted_up_to_the_video_time_without_repeats() {
        let mut s = Silence::default();
        assert_eq!(s.until(0), [0]);
        assert_eq!(s.until(50), [23, 46]);
        assert_eq!(s.until(50), Vec::<u64>::new());
        let ts = s.until(1000);
        assert_eq!(ts.first(), Some(&69));
        assert!(ts.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(ts.last(), Some(&998));
    }

    #[test]
    fn the_silent_frame_is_a_short_fixed_payload() {
        assert_eq!(SILENT_FRAME.len(), 6);
        assert_eq!(AUDIO_SPECIFIC_CONFIG, [0x12, 0x10]);
    }
}
