//! `eq-server bgm-send <MP3 のディレクトリ> <Icecast の URL>`: 前処理済みの MP3 をファイル名順に、再生の速さに合わせて
//! Icecast へ送り続ける (1 周するたびにディレクトリを読み直す)。音は読み解かずにフレームをそのまま送るだけなので軽い。
//! 曲が変わるたびに曲名 (MP3 のタグ。無ければファイル名) を Icecast に知らせる。
//! パスワードは環境変数 ICECAST_SOURCE_PASSWORD。
//!
//! 前処理 (m4a などから配信用の MP3 を作る) は tools/bgm_prepare.sh。

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use lofty::file::TaggedFileExt;
use lofty::tag::Accessor;

pub const USAGE: &str = "usage: eq-server bgm-send <MP3 のディレクトリ> <http://127.0.0.1:8010/bgm.mp3>  (パスワードは ICECAST_SOURCE_PASSWORD)";

/// 再生より先に送っておく時間 (聞き手の手元に貯める分)
const LEAD: Duration = Duration::from_millis(1500);

/// MP3 のフレーム 1 つ (data の中の位置と再生時間)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub start: usize,
    pub len: usize,
    pub micros: u64,
}

/// MP3 (MPEG-1/2/2.5 Layer III) のフレームを並べる。先頭の ID3v2 タグは飛ばし、読めないところは次の同期まで進める
pub fn frames(data: &[u8]) -> Vec<Frame> {
    let mut out = Vec::new();
    let mut i = id3v2_len(data);
    while i + 4 <= data.len() {
        match frame_header(&data[i..i + 4]) {
            Some((len, micros)) if i + len <= data.len() => {
                out.push(Frame { start: i, len, micros });
                i += len;
            }
            _ => i += 1,
        }
    }
    out
}

fn id3v2_len(data: &[u8]) -> usize {
    if data.len() < 10 || &data[..3] != b"ID3" {
        return 0;
    }
    let size = data[6..10]
        .iter()
        .fold(0usize, |acc, b| (acc << 7) | (*b as usize & 0x7f));
    let footer = if data[5] & 0x10 != 0 { 10 } else { 0 };
    10 + size + footer
}

/// フレームの先頭 4 バイトから (フレームの長さ, 再生時間 µs)。Layer III 以外・予約値は None
fn frame_header(h: &[u8]) -> Option<(usize, u64)> {
    if h[0] != 0xFF || h[1] & 0xE0 != 0xE0 {
        return None;
    }
    let version = (h[1] >> 3) & 0x03; // 3: MPEG-1, 2: MPEG-2, 0: MPEG-2.5
    let layer = (h[1] >> 1) & 0x03; // 1: Layer III
    if version == 1 || layer != 1 {
        return None;
    }
    let br_index = (h[2] >> 4) as usize;
    let sr_index = ((h[2] >> 2) & 0x03) as usize;
    let padding = ((h[2] >> 1) & 0x01) as usize;
    if br_index == 0 || br_index == 15 || sr_index == 3 {
        return None;
    }
    const BR_V1: [usize; 15] = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];
    const BR_V2: [usize; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
    const SR: [usize; 3] = [44100, 48000, 32000];
    let (bitrate, samplerate, samples) = match version {
        3 => (BR_V1[br_index], SR[sr_index], 1152),
        2 => (BR_V2[br_index], SR[sr_index] / 2, 576),
        _ => (BR_V2[br_index], SR[sr_index] / 4, 576),
    };
    let len = samples / 8 * bitrate * 1000 / samplerate + padding;
    Some((len, samples as u64 * 1_000_000 / samplerate as u64))
}

/// "http://host:port/mount" を (host:port, /mount) に
fn parse_url(url: &str) -> anyhow::Result<(String, String)> {
    let rest = url
        .strip_prefix("http://")
        .context("Icecast の URL は http:// で始める")?;
    let (host, path) = rest
        .split_once('/')
        .context("Icecast の URL にマウント (/bgm.mp3 など) が無い")?;
    let host = if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:80")
    };
    Ok((host, format!("/{path}")))
}

fn base64(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in input.chunks(3) {
        let n = c
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= c.len() {
                T[(n >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

fn percent(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// 曲名: タグの「アーティスト - タイトル」、無ければファイル名 (拡張子なし)
fn song_name(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    // 曲名のタグだけを読む (再生時間などは読まない)
    let tagged = lofty::probe::Probe::open(path)
        .map(|p| p.options(lofty::config::ParseOptions::new().read_properties(false)))
        .and_then(|p| p.read())
        .ok();
    let tag = tagged.as_ref().and_then(|t| t.primary_tag().or_else(|| t.first_tag()));
    let title = tag
        .and_then(|t| t.title())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let artist = tag
        .and_then(|t| t.artist())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    match (artist, title) {
        (Some(a), Some(t)) => format!("{a} - {t}"),
        (None, Some(t)) => t,
        _ => stem,
    }
}

fn mp3_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    files.retain(|p| {
        p.is_file()
            && p.extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| x.eq_ignore_ascii_case("mp3"))
            && !p.file_name().and_then(|n| n.to_str()).unwrap_or(".").starts_with('.')
    });
    files.sort();
    files
}

struct Target {
    host: String,
    mount: String,
    auth: String,
}

impl Target {
    /// 送り口を開く (HTTP PUT)。以後は本文として MP3 を書き続ける
    fn connect(&self) -> anyhow::Result<TcpStream> {
        let mut s = TcpStream::connect(&self.host).with_context(|| format!("connecting {}", self.host))?;
        write!(
            s,
            "PUT {} HTTP/1.1\r\nHost: {}\r\nAuthorization: Basic {}\r\nUser-Agent: eq-server bgm-send\r\n\
             Content-Type: audio/mpeg\r\nIce-Name: eq-webservice BGM\r\nIce-Public: 0\r\nExpect: 100-continue\r\n\r\n",
            self.mount, self.host, self.auth
        )?;
        let status = read_status(&mut s)?;
        if !(status == 100 || status == 200) {
            bail!("Icecast refused the source: HTTP {status}");
        }
        s.set_read_timeout(None)?;
        Ok(s)
    }

    /// 今の曲名を知らせる (失敗しても送り続ける)
    fn set_song(&self, song: &str) {
        let r = (|| -> anyhow::Result<()> {
            let mut s = TcpStream::connect(&self.host)?;
            write!(
                s,
                "GET /admin/metadata?mount={}&mode=updinfo&charset=UTF-8&song={} HTTP/1.0\r\nHost: {}\r\n\
                 Authorization: Basic {}\r\nUser-Agent: eq-server bgm-send\r\n\r\n",
                percent(&self.mount),
                percent(song),
                self.host,
                self.auth
            )?;
            let status = read_status(&mut s)?;
            if status != 200 {
                bail!("HTTP {status}");
            }
            Ok(())
        })();
        if let Err(e) = r {
            tracing::warn!("bgm-send: metadata: {e:#}");
        }
    }
}

/// 応答の最初の行から状態コードを読む (5 秒で諦める)
fn read_status(s: &mut TcpStream) -> anyhow::Result<u16> {
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut buf = [0u8; 512];
    let n = s.read(&mut buf).context("no response from Icecast")?;
    let head = String::from_utf8_lossy(&buf[..n]);
    head.split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .with_context(|| format!("unexpected response: {head:?}"))
}

pub fn run(args: &[String]) -> anyhow::Result<()> {
    let [dir, url] = args else { bail!("{USAGE}") };
    let password = std::env::var("ICECAST_SOURCE_PASSWORD").context("ICECAST_SOURCE_PASSWORD is not set")?;
    let (host, mount) = parse_url(url)?;
    let target = Target {
        host,
        mount,
        auth: base64(format!("source:{password}").as_bytes()),
    };
    let dir = PathBuf::from(dir);
    let mut last: Option<PathBuf> = None;
    loop {
        match stream(&target, &dir, &mut last) {
            Ok(()) => {}
            Err(e) => tracing::warn!("bgm-send: {e:#}"),
        }
        std::thread::sleep(Duration::from_secs(5));
    }
}

/// 1 回つないで送り続ける (切れたら Err)。last は最後に送り始めた曲 (つなぎ直したらその次から)
fn stream(target: &Target, dir: &Path, last: &mut Option<PathBuf>) -> anyhow::Result<()> {
    let mut conn = target.connect()?;
    tracing::info!(host = %target.host, mount = %target.mount, "bgm-send connected");
    let start = Instant::now();
    let mut sent = Duration::ZERO;
    loop {
        // 1 曲ごとに読み直す (差し替えた曲もその次から入る)
        let files = mp3_files(dir);
        let Some(next) = files
            .iter()
            .find(|f| last.as_ref().is_none_or(|l| *f > l))
            .or(files.first())
            .cloned()
        else {
            std::thread::sleep(Duration::from_secs(30));
            continue;
        };
        *last = Some(next.clone());
        let data = std::fs::read(&next).with_context(|| format!("reading {}", next.display()))?;
        let fs = frames(&data);
        if fs.is_empty() {
            tracing::warn!(file = %next.display(), "bgm-send: no MP3 frames, skipped");
            continue;
        }
        let song = song_name(&next);
        tracing::info!(%song, "bgm-send now playing");
        target.set_song(&song);
        for f in fs {
            conn.write_all(&data[f.start..f.start + f.len])?;
            sent += Duration::from_micros(f.micros);
            // 再生より LEAD 以上先に進んだら待つ
            let ahead = sent.saturating_sub(start.elapsed());
            if ahead > LEAD {
                std::thread::sleep(ahead - LEAD);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MPEG-1 Layer III, 128kbps, 44.1kHz のフレーム (padding なし: 417 バイト)
    fn frame() -> Vec<u8> {
        let mut f = vec![0xFF, 0xFB, 0x90, 0x00];
        f.resize(417, 0);
        f
    }

    #[test]
    fn frames_are_found_after_an_id3_tag_and_timed() {
        let mut data = b"ID3\x03\x00\x00\x00\x00\x00\x05hello".to_vec();
        for _ in 0..3 {
            data.extend(frame());
        }
        data.extend(b"TAG trailing id3v1 is ignored");
        let fs = frames(&data);
        assert_eq!(fs.len(), 3);
        assert_eq!(
            fs[0],
            Frame {
                start: 15,
                len: 417,
                micros: 26_122
            }
        );
        assert_eq!(fs[2].start, 15 + 417 * 2);
    }

    #[test]
    fn garbage_between_frames_is_skipped() {
        let mut data = frame();
        data.extend([0x00, 0x12, 0xFF, 0x00]);
        data.extend(frame());
        assert_eq!(frames(&data).len(), 2);
    }

    #[test]
    fn url_and_auth() {
        assert_eq!(
            parse_url("http://127.0.0.1:8010/bgm.mp3").unwrap(),
            ("127.0.0.1:8010".into(), "/bgm.mp3".into())
        );
        assert!(parse_url("https://x/bgm.mp3").is_err());
        assert_eq!(base64(b"source:hackme"), "c291cmNlOmhhY2ttZQ==");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(percent("A - B/曲"), "A%20-%20B%2F%E6%9B%B2");
    }
}
