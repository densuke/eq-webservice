use super::*;

fn cfg(toml: &str) -> BroadcastConfig {
    toml::from_str(toml).unwrap()
}

#[test]
fn ffmpeg_is_the_default_encoder() {
    assert_eq!(BroadcastConfig::default().encoder, super::super::EncoderKind::Ffmpeg);
}

#[test]
fn builtin_needs_native_and_silence() {
    assert!(BuiltinEncoder::check(&cfg("encoder = \"builtin\"\nsource = \"native\"")).is_ok());
    assert!(BuiltinEncoder::check(&cfg("encoder = \"builtin\"")).is_err());
    assert!(BuiltinEncoder::check(&cfg("encoder = \"builtin\"\nsource = \"native\"\nmixer = true")).is_err());
    assert!(BuiltinEncoder::check(&cfg(
        "encoder = \"builtin\"\nsource = \"native\"\naudio = [\"-i\", \"x\"]"
    ))
    .is_err());
}

#[tokio::test]
async fn a_file_gets_flv_header_config_tags_and_time_ordered_frames() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.flv");
    let c = cfg("source = \"native\"\nwidth = 320\nheight = 180\nfps = 10");
    let mut e = BuiltinEncoder::start(&c, &[path.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let mut img = vec![100u8; 320 * 180];
    img.extend(vec![128u8; 320 * 180 / 2]);
    for t in [0u64, 500, 1000, 2000] {
        e.video(&img, t).await.unwrap();
    }
    let f = std::fs::read(&path).unwrap();
    assert_eq!(&f[..3], b"FLV");
    // タグを順にたどり、時刻が減らないこと・映像のキーフレームが 0ms と 2000ms にあることを確かめる
    let (mut i, mut last, mut keys) = (13, 0u32, Vec::new());
    while i < f.len() {
        let len = u32::from_be_bytes([0, f[i + 1], f[i + 2], f[i + 3]]) as usize;
        let ts = u32::from_be_bytes([f[i + 7], f[i + 4], f[i + 5], f[i + 6]]);
        assert!(ts >= last, "時刻が戻った");
        last = ts;
        if f[i] == flv::TAG_VIDEO && f[i + 11] == 0x17 && f[i + 12] == 1 {
            keys.push(ts);
        }
        i += 11 + len + 4;
    }
    assert_eq!(i, f.len());
    assert_eq!(keys, [0, 2000]);
}
