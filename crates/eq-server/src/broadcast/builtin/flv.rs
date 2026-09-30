//! FLV のヘッダとタグ。RTMP のメッセージの本体も FLV のタグの本体と同じ形なので、`*_body` は両方に使う。

use super::amf::{self, Value};

pub const TAG_AUDIO: u8 = 8;
pub const TAG_VIDEO: u8 = 9;
pub const TAG_DATA: u8 = 18;

/// FLV のヘッダ (映像と音あり) と最初の前タグ長 (0)
pub fn header() -> Vec<u8> {
    vec![b'F', b'L', b'V', 1, 0x05, 0, 0, 0, 9, 0, 0, 0, 0]
}

/// タグ 1 つ (11 バイトのヘッダ + 本体 + 前タグ長)。時刻は 24 ビット + 拡張 8 ビット
pub fn tag(kind: u8, ts: u32, body: &[u8]) -> Vec<u8> {
    let len = body.len() as u32;
    let mut out = Vec::with_capacity(15 + body.len());
    out.push(kind);
    out.extend_from_slice(&len.to_be_bytes()[1..]);
    out.extend_from_slice(&ts.to_be_bytes()[1..]);
    out.push((ts >> 24) as u8);
    out.extend_from_slice(&[0, 0, 0]);
    out.extend_from_slice(body);
    out.extend_from_slice(&(11 + len).to_be_bytes());
    out
}

/// onMetaData の本体。RTMP で送るときは先頭に @setDataFrame が付く
pub fn metadata_body(width: u32, height: u32, fps: u32, video_kbps: u32, rtmp: bool) -> Vec<u8> {
    let mut v = Vec::new();
    if rtmp {
        v.push(amf::s("@setDataFrame"));
    }
    v.push(amf::s("onMetaData"));
    v.push(Value::Obj(vec![
        ("width".into(), Value::Num(width as f64)),
        ("height".into(), Value::Num(height as f64)),
        ("framerate".into(), Value::Num(fps as f64)),
        ("videocodecid".into(), Value::Num(7.0)),
        ("videodatarate".into(), Value::Num(video_kbps as f64)),
        ("audiocodecid".into(), Value::Num(10.0)),
        ("audiosamplerate".into(), Value::Num(44100.0)),
        ("audiosamplesize".into(), Value::Num(16.0)),
        ("stereo".into(), Value::Bool(true)),
        ("encoder".into(), amf::s("eq-server")),
    ]));
    amf::write_all(&v)
}

/// AVCDecoderConfigurationRecord (SPS・PPS は開始符号なし)
pub fn avc_config(sps: &[u8], pps: &[u8]) -> Vec<u8> {
    let mut out = vec![1, sps[1], sps[2], sps[3], 0xff, 0xe1];
    out.extend_from_slice(&(sps.len() as u16).to_be_bytes());
    out.extend_from_slice(sps);
    out.push(1);
    out.extend_from_slice(&(pps.len() as u16).to_be_bytes());
    out.extend_from_slice(pps);
    out
}

/// 映像の本体: AVC のシーケンスヘッダ
pub fn video_config_body(sps: &[u8], pps: &[u8]) -> Vec<u8> {
    let mut out = vec![0x17, 0, 0, 0, 0];
    out.extend(avc_config(sps, pps));
    out
}

/// 映像の本体: 1 コマ (avcc は長さ前置きの NAL の並び)。並べ替えは無い (B フレームなし) ので合成時刻は 0
pub fn video_body(keyframe: bool, avcc: &[u8]) -> Vec<u8> {
    let mut out = vec![if keyframe { 0x17 } else { 0x27 }, 1, 0, 0, 0];
    out.extend_from_slice(avcc);
    out
}

/// 音の本体: AAC のシーケンスヘッダ
pub fn audio_config_body(config: &[u8]) -> Vec<u8> {
    let mut out = vec![0xaf, 0];
    out.extend_from_slice(config);
    out
}

/// 音の本体: AAC の 1 コマ (AAC・44kHz・16bit・ステレオ)
pub fn audio_body(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0xaf, 1];
    out.extend_from_slice(raw);
    out
}

/// Annex B (開始符号 00 00 01 / 00 00 00 01 で区切る) を NAL に分ける
pub fn split_annex_b(data: &[u8]) -> Vec<&[u8]> {
    // 開始符号の直後の位置を集める
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        if data[i..i + 3] == [0, 0, 1] {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    starts
        .iter()
        .enumerate()
        .map(|(n, &s)| {
            let mut e = starts.get(n + 1).map_or(data.len(), |&next| next - 3);
            // 4 バイトの開始符号の先頭の 0 は前の NAL に含めない
            while e > s && data[e - 1] == 0 && n + 1 < starts.len() {
                e -= 1;
            }
            &data[s..e]
        })
        .collect()
}

/// NAL の種類 (下位 5 ビット)
pub fn nal_type(nal: &[u8]) -> u8 {
    nal.first().map_or(0, |b| b & 0x1f)
}

/// NAL を長さ (4 バイト) 前置きにして並べる
pub fn to_avcc(nals: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for n in nals {
        out.extend_from_slice(&(n.len() as u32).to_be_bytes());
        out.extend_from_slice(n);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_is_flv_with_audio_and_video() {
        let h = header();
        assert_eq!(&h[..5], b"FLV\x01\x05");
        assert_eq!(&h[5..9], [0, 0, 0, 9]);
        // 最初の前タグ長は 0
        assert_eq!(&h[9..], [0, 0, 0, 0]);
    }

    #[test]
    fn tag_has_length_time_and_previous_tag_size() {
        let t = tag(TAG_VIDEO, 0x0102_0304, &[9, 8, 7]);
        assert_eq!(t[0], 9);
        assert_eq!(&t[1..4], [0, 0, 3]);
        // 時刻は下位 24 ビット、その次に上位 8 ビット
        assert_eq!(&t[4..7], [0x02, 0x03, 0x04]);
        assert_eq!(t[7], 0x01);
        assert_eq!(&t[8..11], [0, 0, 0]);
        assert_eq!(&t[11..14], [9, 8, 7]);
        // 前タグ長 = 11 + 本体
        assert_eq!(&t[14..], 14u32.to_be_bytes());
        assert_eq!(t.len(), 11 + 3 + 4);
    }

    #[test]
    fn avc_config_carries_profile_sps_and_pps() {
        let sps = [0x67, 0x42, 0xc0, 0x1f, 0xaa];
        let pps = [0x68, 0xce, 0x3c];
        let c = avc_config(&sps, &pps);
        // version、profile・互換・level は SPS の 2〜4 バイト目、NAL の長さは 4 バイト、SPS は 1 つ
        assert_eq!(&c[..6], [1, 0x42, 0xc0, 0x1f, 0xff, 0xe1]);
        assert_eq!(&c[6..8], [0, 5]);
        assert_eq!(&c[8..13], sps);
        assert_eq!(c[13], 1);
        assert_eq!(&c[14..16], [0, 3]);
        assert_eq!(&c[16..], pps);
        let b = video_config_body(&sps, &pps);
        assert_eq!(&b[..5], [0x17, 0, 0, 0, 0]);
    }

    #[test]
    fn video_body_marks_keyframes() {
        assert_eq!(&video_body(true, &[1, 2])[..5], [0x17, 1, 0, 0, 0]);
        assert_eq!(&video_body(false, &[1, 2])[..5], [0x27, 1, 0, 0, 0]);
        assert_eq!(&audio_body(&[5])[..], [0xaf, 1, 5]);
        assert_eq!(&audio_config_body(&[0x12, 0x10])[..], [0xaf, 0, 0x12, 0x10]);
    }

    #[test]
    fn annex_b_is_split_and_converted_to_length_prefixed() {
        let data = [0, 0, 0, 1, 0x67, 1, 2, 0, 0, 1, 0x68, 3, 0, 0, 0, 1, 0x65, 4, 5, 6];
        let nals = split_annex_b(&data);
        assert_eq!(nals, [&[0x67, 1, 2][..], &[0x68, 3][..], &[0x65, 4, 5, 6][..]]);
        assert_eq!(nals.iter().map(|n| nal_type(n)).collect::<Vec<_>>(), [7, 8, 5]);
        assert_eq!(to_avcc(&nals[1..2]), [0, 0, 0, 2, 0x68, 3]);
    }

    #[test]
    fn metadata_is_named_and_rtmp_adds_set_data_frame() {
        let m = amf::read_all(&metadata_body(1280, 720, 10, 300, false));
        assert_eq!(m[0].as_str(), Some("onMetaData"));
        assert_eq!(m[1].get("width").and_then(Value::as_num), Some(1280.0));
        let r = amf::read_all(&metadata_body(1280, 720, 10, 300, true));
        assert_eq!(r[0].as_str(), Some("@setDataFrame"));
        assert_eq!(r[1].as_str(), Some("onMetaData"));
    }
}
