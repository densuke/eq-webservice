//! 音声アナウンス用の WAV (モノラル 44.1kHz 16bit PCM) の読み書きと連結。docs/tts.md S3 を参照

use anyhow::{bail, Context};

/// サンプリングレート (Hz)
pub const RATE: u32 = 44_100;

/// WAV を検証して data チャンクのサンプルを返す。
pub fn parse(bytes: &[u8]) -> anyhow::Result<Vec<i16>> {
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        bail!("RIFF/WAVE ではありません");
    }
    let mut pos = 12;
    let mut fmt_ok = false;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into()?) as usize;
        let body = bytes
            .get(pos + 8..pos + 8 + size)
            .with_context(|| format!("チャンクが途中で切れています: {}", String::from_utf8_lossy(id)))?;
        match id {
            b"fmt " => {
                let f = body.get(0..16).context("fmt チャンクが短すぎます")?;
                let u16_at = |i: usize| u16::from_le_bytes([f[i], f[i + 1]]);
                let rate = u32::from_le_bytes([f[4], f[5], f[6], f[7]]);
                if u16_at(0) != 1 || u16_at(2) != 1 || rate != RATE || u16_at(14) != 16 {
                    bail!(
                        "非対応の WAV です (format={}, channels={}, rate={}, bits={}): モノラル 44100Hz 16bit PCM のみ",
                        u16_at(0),
                        u16_at(2),
                        rate,
                        u16_at(14)
                    );
                }
                fmt_ok = true;
            }
            b"data" => {
                if !fmt_ok {
                    bail!("fmt チャンクより前に data チャンクがあります");
                }
                return Ok(body.as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes(*c)).collect());
            }
            _ => {}
        }
        pos += 8 + size + (size & 1);
    }
    bail!("data チャンクがありません")
}

/// 44 バイトの標準ヘッダ付き WAV に書き出す。
pub fn encode(pcm: &[i16]) -> Vec<u8> {
    encode_at(pcm, RATE)
}

/// ブラウザ向けに半分の点数 (22.05kHz) へ落とす。読み上げは声だけなので十分聞き取れ、
/// 遠い回線 (n2 は米国西部) でも届くまでの時間が半分になる。隣り合う 2 点の平均で、端の 1 点は捨てる
pub fn half_rate(wav: &[u8]) -> anyhow::Result<Vec<u8>> {
    Ok(encode_half(&parse(wav)?))
}

/// PCM を半分の点数 (22.05kHz) にして WAV に書く。
pub fn encode_half(pcm: &[i16]) -> Vec<u8> {
    let mut sink = Sink::new(pcm.len(), RATE / 2);
    sink.extend(pcm);
    sink.finish()
}

fn encode_at(pcm: &[i16], rate: u32) -> Vec<u8> {
    let mut sink = Sink::new(pcm.len(), rate);
    sink.extend(pcm);
    sink.finish()
}

/// 44 バイトの標準ヘッダ (data は data_len バイト)
fn header(data_len: u32, rate: u32) -> [u8; 44] {
    let mut h = Vec::with_capacity(44);
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&(36 + data_len).to_le_bytes());
    h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&16u32.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes()); // PCM
    h.extend_from_slice(&1u16.to_le_bytes()); // モノラル
    h.extend_from_slice(&rate.to_le_bytes());
    h.extend_from_slice(&(rate * 2).to_le_bytes()); // byte rate
    h.extend_from_slice(&2u16.to_le_bytes()); // block align
    h.extend_from_slice(&16u16.to_le_bytes());
    h.extend_from_slice(b"data");
    h.extend_from_slice(&data_len.to_le_bytes());
    h.try_into().unwrap()
}

/// WAV の出力を 1 つのバッファに直接書き足す。PCM を連結した大きな Vec<i16> を作らずに済む。
/// 先頭にヘッダを置き、`finish` で実際の長さに直す (見込みより短くなっても正しい)。
/// 半分の点数 (22.05kHz) では、隣り合う 2 点の平均を書く。部品の境目をまたぐ組も平均する (連結してから落とすのと同じ)
pub struct Sink {
    out: Vec<u8>,
    rate: u32,
    /// 半分にするときの、平均の相手待ちの 1 点
    pending: Option<i16>,
}

impl Sink {
    /// samples は、入れる点数の見込み (バッファの確保にだけ使う)。rate は RATE か RATE / 2
    pub fn new(samples: usize, rate: u32) -> Self {
        let per_out = if rate == RATE { 1 } else { 2 };
        let mut out = Vec::with_capacity(44 + samples / per_out * 2);
        out.extend_from_slice(&header(0, rate));
        Sink {
            out,
            rate,
            pending: None,
        }
    }

    /// 入力は常に 44.1kHz の点
    pub fn extend(&mut self, pcm: &[i16]) {
        if self.rate == RATE {
            self.out.extend(pcm.iter().flat_map(|s| s.to_le_bytes()));
            return;
        }
        for &s in pcm {
            match self.pending.take() {
                Some(a) => self
                    .out
                    .extend_from_slice(&(((a as i32 + s as i32) / 2) as i16).to_le_bytes()),
                None => self.pending = Some(s),
            }
        }
    }

    /// 無音を n 点入れる
    pub fn silence(&mut self, n: usize) {
        for _ in 0..n {
            self.extend(&[0]);
        }
    }

    /// ヘッダの長さを直して返す (相手待ちの端の 1 点は捨てる)
    pub fn finish(mut self) -> Vec<u8> {
        let data_len = (self.out.len() - 44) as u32;
        self.out[..44].copy_from_slice(&header(data_len, self.rate));
        self.out
    }
}

/// 部品の間にだけ gap_ms の無音を挟んで連結する。
pub fn join(parts: &[Vec<i16>], gap_ms: u32) -> Vec<i16> {
    let gap = vec![0i16; (RATE as u64 * gap_ms as u64 / 1000) as usize];
    parts.iter().map(Vec::as_slice).collect::<Vec<_>>().join(gap.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;

    // 実装に依存しない手書きの WAV ビルダ。(fmt チャンクと、data の前に挟む追加チャンク)
    fn build(tag: u16, ch: u16, rate: u32, bits: u16, extra: &[u8], pcm: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut body = Vec::new();
        body.extend_from_slice(b"WAVE");
        body.extend_from_slice(b"fmt ");
        body.extend_from_slice(&16u32.to_le_bytes());
        body.extend_from_slice(&tag.to_le_bytes());
        body.extend_from_slice(&ch.to_le_bytes());
        body.extend_from_slice(&rate.to_le_bytes());
        let block = ch * bits / 8;
        body.extend_from_slice(&(rate * block as u32).to_le_bytes());
        body.extend_from_slice(&block.to_le_bytes());
        body.extend_from_slice(&bits.to_le_bytes());
        body.extend_from_slice(extra);
        body.extend_from_slice(b"data");
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(&data);
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    fn good(pcm: &[i16]) -> Vec<u8> {
        build(1, 1, 44_100, 16, &[], pcm)
    }

    #[test]
    fn handwritten_helper_is_parsed() {
        assert_eq!(parse(&good(&[1, 2, 3])).unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn roundtrip_keeps_extremes() {
        let pcm = [0, 1, -1, i16::MAX, i16::MIN];
        assert_eq!(parse(&encode(&pcm)).unwrap(), pcm);
    }

    #[test]
    fn encode_has_standard_44_byte_header() {
        let bytes = encode(&[1, 2]);
        assert_eq!(bytes.len(), 44 + 4);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
    }

    #[test]
    fn empty_roundtrip() {
        assert!(parse(&encode(&[])).unwrap().is_empty());
    }

    #[test]
    fn stereo_is_rejected() {
        assert!(parse(&build(1, 2, 44_100, 16, &[], &[0, 0])).is_err());
    }

    #[test]
    fn wrong_rate_is_rejected() {
        assert!(parse(&build(1, 1, 22_050, 16, &[], &[0, 0])).is_err());
    }

    #[test]
    fn eight_bit_is_rejected() {
        assert!(parse(&build(1, 1, 44_100, 8, &[], &[0, 0])).is_err());
    }

    #[test]
    fn non_pcm_tag_is_rejected() {
        assert!(parse(&build(3, 1, 44_100, 16, &[], &[0, 0])).is_err());
    }

    #[test]
    fn garbage_is_rejected_without_panic() {
        assert!(parse(b"").is_err());
        assert!(parse(b"not a wav file at all, really").is_err());
        let full = good(&[1, 2, 3, 4]);
        for n in [4, 12, 20, 30, 40] {
            // 途中で切れていても panic せず、Ok でも Err でもよいが落ちないこと
            let _ = parse(&full[..n]);
        }
        assert!(parse(&full[..12]).is_err());
    }

    #[test]
    fn odd_length_list_chunk_is_skipped() {
        // LIST は 3 バイト + パディング 1 バイト (ワード境界)
        let mut extra = b"LIST".to_vec();
        extra.extend_from_slice(&3u32.to_le_bytes());
        extra.extend_from_slice(&[b'a', b'b', b'c', 0]);
        let bytes = build(1, 1, 44_100, 16, &extra, &[5, -5, 7]);
        assert_eq!(parse(&bytes).unwrap(), vec![5, -5, 7]);
    }

    #[test]
    fn join_inserts_gap_only_between_parts() {
        let parts = vec![vec![1i16; 10], vec![2; 20], vec![3; 5]];
        let gap = (RATE as usize) * 150 / 1000;
        assert_eq!(gap, 6615);
        let out = join(&parts, 150);
        assert_eq!(out.len(), 35 + gap * 2);
        assert_eq!(&out[..10], &[1; 10]);
        assert!(out[10..10 + gap].iter().all(|&s| s == 0));
        assert_eq!(&out[10 + gap..30 + gap], &[2; 20]);
        assert!(out[30 + gap..30 + 2 * gap].iter().all(|&s| s == 0));
        assert_eq!(&out[30 + 2 * gap..], &[3; 5]);
    }

    #[test]
    fn join_empty_is_empty() {
        assert!(join(&[], 150).is_empty());
    }

    #[test]
    fn join_single_part_is_unchanged() {
        assert_eq!(join(&[vec![9, 8, 7]], 150), vec![9, 8, 7]);
    }

    #[test]
    fn half_rate_averages_pairs_and_writes_22050_hz() {
        // ブラウザ向けに半分の点数へ落とす (隣り合う 2 点の平均)。端の 1 点は捨てる
        let half = half_rate(&encode(&[10, 20, 30, 40, 5])).unwrap();
        assert_eq!(u32::from_le_bytes(half[24..28].try_into().unwrap()), 22_050);
        assert_eq!(u32::from_le_bytes(half[28..32].try_into().unwrap()), 44_100); // byte rate
        let data: Vec<i16> = half[44..].chunks(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(data, vec![15, 35]);
        assert_eq!(u32::from_le_bytes(half[40..44].try_into().unwrap()), 4);
    }

    #[test]
    fn half_rate_does_not_overflow_on_loud_samples() {
        let half = half_rate(&encode(&[i16::MAX, i16::MAX, i16::MIN, i16::MIN])).unwrap();
        let data: Vec<i16> = half[44..].chunks(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(data, vec![i16::MAX, i16::MIN]);
    }

    #[test]
    fn half_rate_rejects_broken_input() {
        assert!(half_rate(b"not a wav").is_err());
    }

    // 以前の実装 (PCM を全部つないでから書く)。Sink の出力がバイト単位で同じことの基準
    fn old_encode_at(pcm: &[i16], rate: u32) -> Vec<u8> {
        let data_len = (pcm.len() * 2) as u32;
        let mut out = Vec::with_capacity(44 + pcm.len() * 2);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.extend(pcm.iter().flat_map(|s| s.to_le_bytes()));
        out
    }

    fn old_encode_half(pcm: &[i16]) -> Vec<u8> {
        let half: Vec<i16> = pcm
            .as_chunks::<2>()
            .0
            .iter()
            .map(|[a, b]| ((*a as i32 + *b as i32) / 2) as i16)
            .collect();
        old_encode_at(&half, RATE / 2)
    }

    fn sample_parts() -> Vec<Vec<i16>> {
        // 奇数長・偶数長・空・極端な値をまぜる (部品の境目で組がずれる)
        vec![
            vec![1, -2, 3],
            vec![i16::MAX, i16::MIN],
            vec![],
            vec![7, 8, 9, 10, 11],
            vec![-5],
        ]
    }

    #[test]
    fn sink_output_is_byte_identical_to_the_old_join_then_encode() {
        let parts = sample_parts();
        for gap_ms in [0u32, 150] {
            let gap = (RATE as u64 * gap_ms as u64 / 1000) as usize;
            let nonempty: Vec<&Vec<i16>> = parts.iter().collect();
            let all = join(&parts, gap_ms);
            for half in [false, true] {
                let rate = if half { RATE / 2 } else { RATE };
                let mut sink = Sink::new(1, rate); // 見込み違いでも正しい
                for (i, p) in nonempty.iter().enumerate() {
                    if i > 0 {
                        sink.silence(gap);
                    }
                    sink.extend(p);
                }
                let want = if half {
                    old_encode_half(&all)
                } else {
                    old_encode_at(&all, RATE)
                };
                assert_eq!(sink.finish(), want, "gap_ms={gap_ms} half={half}");
            }
        }
    }

    #[test]
    fn encode_and_encode_half_are_unchanged() {
        for pcm in [
            vec![],
            vec![1],
            vec![1, 2],
            vec![i16::MAX, i16::MAX, i16::MIN, i16::MIN, 3],
        ] {
            assert_eq!(encode(&pcm), old_encode_at(&pcm, RATE));
            assert_eq!(encode_half(&pcm), old_encode_half(&pcm));
        }
    }
}
