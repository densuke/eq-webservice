//! RTMP のチャンク (メッセージを chunk_size ごとに区切って送る形) の組み立てと読み込み。

use std::collections::HashMap;

use anyhow::Context;
use tokio::io::{AsyncRead, AsyncReadExt};

/// 送るときのチャンクの大きさ (最初は 128。接続の最初に Set Chunk Size で伝える)
pub const SEND_CHUNK: usize = 4096;
/// 受け取るメッセージの大きさの上限 (サーバの応答は小さい)
const MAX_MESSAGE: usize = 1 << 20;

pub const MSG_SET_CHUNK_SIZE: u8 = 1;
pub const MSG_ACK: u8 = 3;
pub const MSG_USER_CONTROL: u8 = 4;
pub const MSG_ACK_WINDOW: u8 = 5;
pub const MSG_COMMAND: u8 = 20;

/// メッセージ 1 つを、最初は完全なヘッダ (fmt 0)、続きは fmt 3 のチャンクにして並べる
pub fn chunks(csid: u8, ts: u32, kind: u8, msid: u32, body: &[u8]) -> Vec<u8> {
    let ext = ts >= 0xff_ffff;
    let field = if ext { 0xff_ffff } else { ts };
    let mut out = Vec::with_capacity(body.len() + 16 + body.len() / SEND_CHUNK * 5);
    out.push(csid & 0x3f);
    out.extend_from_slice(&field.to_be_bytes()[1..]);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
    out.push(kind);
    out.extend_from_slice(&msid.to_le_bytes());
    if ext {
        out.extend_from_slice(&ts.to_be_bytes());
    }
    let mut parts = body.chunks(SEND_CHUNK);
    out.extend_from_slice(parts.next().unwrap_or(&[]));
    for p in parts {
        out.push(0xc0 | (csid & 0x3f));
        if ext {
            out.extend_from_slice(&ts.to_be_bytes());
        }
        out.extend_from_slice(p);
    }
    out
}

/// Set Chunk Size (チャンクストリーム 2 の制御メッセージ)
pub fn set_chunk_size(n: u32) -> Vec<u8> {
    chunks(2, 0, MSG_SET_CHUNK_SIZE, 0, &n.to_be_bytes())
}

pub fn ack(received: u32) -> Vec<u8> {
    chunks(2, 0, MSG_ACK, 0, &received.to_be_bytes())
}

/// PingRequest (ユーザー制御のイベント 6) への返事 (イベント 7)
pub fn ping_response(request_body: &[u8]) -> Vec<u8> {
    let mut b = vec![0, 7];
    b.extend_from_slice(request_body.get(2..6).unwrap_or(&[0; 4]));
    chunks(2, 0, MSG_USER_CONTROL, 0, &b)
}

/// 受け取ったメッセージ
#[derive(Debug)]
pub struct Message {
    pub kind: u8,
    pub body: Vec<u8>,
}

#[derive(Default)]
struct Partial {
    ts: u32,
    len: usize,
    kind: u8,
    ext: bool,
    buf: Vec<u8>,
}

#[derive(Default)]
pub struct Reader {
    chunk_size: usize,
    streams: HashMap<u32, Partial>,
    /// ここまでに読んだバイト数 (Acknowledgement に使う)
    pub received: u64,
}

impl Reader {
    pub fn new() -> Self {
        Self {
            chunk_size: 128,
            ..Self::default()
        }
    }

    async fn byte<R: AsyncRead + Unpin>(&mut self, r: &mut R, n: usize) -> anyhow::Result<Vec<u8>> {
        let mut b = vec![0; n];
        r.read_exact(&mut b).await.context("RTMP の受信")?;
        self.received += n as u64;
        Ok(b)
    }

    /// メッセージが 1 つ揃うまで読む (Set Chunk Size は読んだ時点で反映する)
    pub async fn next<R: AsyncRead + Unpin>(&mut self, r: &mut R) -> anyhow::Result<Message> {
        loop {
            let b0 = self.byte(r, 1).await?[0];
            let fmt = b0 >> 6;
            let csid = match b0 & 0x3f {
                0 => 64 + self.byte(r, 1).await?[0] as u32,
                1 => {
                    let b = self.byte(r, 2).await?;
                    64 + b[0] as u32 + b[1] as u32 * 256
                }
                n => n as u32,
            };
            let st = self.streams.entry(csid).or_default();
            let mut st_taken = std::mem::take(st);
            let head = [11usize, 7, 3, 0][fmt as usize];
            let h = {
                let mut b = vec![0; head];
                r.read_exact(&mut b).await.context("RTMP の受信")?;
                self.received += head as u64;
                b
            };
            if fmt <= 2 {
                let v = u32::from_be_bytes([0, h[0], h[1], h[2]]);
                st_taken.ext = v == 0xff_ffff;
                st_taken.ts = if fmt == 0 { v } else { st_taken.ts.wrapping_add(v) };
            }
            if fmt <= 1 {
                st_taken.len = u32::from_be_bytes([0, h[3], h[4], h[5]]) as usize;
                st_taken.kind = h[6];
                anyhow::ensure!(st_taken.len <= MAX_MESSAGE, "RTMP のメッセージが大きすぎます");
            }
            if st_taken.ext {
                let e = self.byte(r, 4).await?;
                if fmt == 0 {
                    st_taken.ts = u32::from_be_bytes([e[0], e[1], e[2], e[3]]);
                }
            }
            let want = self.chunk_size.min(st_taken.len.saturating_sub(st_taken.buf.len()));
            let data = self.byte(r, want).await?;
            st_taken.buf.extend_from_slice(&data);
            if st_taken.buf.len() >= st_taken.len {
                let body = std::mem::take(&mut st_taken.buf);
                let kind = st_taken.kind;
                self.streams.insert(csid, st_taken);
                if kind == MSG_SET_CHUNK_SIZE && body.len() >= 4 {
                    let n = u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize;
                    self.chunk_size = n.clamp(1, MAX_MESSAGE);
                }
                return Ok(Message { kind, body });
            }
            self.streams.insert(csid, st_taken);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_message_is_one_full_header_chunk() {
        let c = chunks(4, 1000, 8, 1, &[1, 2, 3]);
        // 基本ヘッダ、時刻、長さ、種類、ストリーム ID (リトルエンディアン)、本体
        assert_eq!(c, [4, 0, 3, 0xe8, 0, 0, 3, 8, 1, 0, 0, 0, 1, 2, 3]);
    }

    #[test]
    fn a_long_message_is_split_with_type_3_continuations() {
        let body = vec![7u8; SEND_CHUNK * 2 + 10];
        let c = chunks(6, 5, 9, 1, &body);
        assert_eq!(c.len(), 12 + body.len() + 2);
        assert_eq!(c[12 + SEND_CHUNK], 0xc0 | 6);
        assert_eq!(c[12 + SEND_CHUNK + 1 + SEND_CHUNK], 0xc0 | 6);
    }

    #[test]
    fn a_time_over_24_bits_uses_the_extended_field_in_every_chunk() {
        let ts = 0x0123_4567;
        let body = vec![0u8; SEND_CHUNK + 1];
        let c = chunks(6, ts, 9, 1, &body);
        assert_eq!(&c[1..4], [0xff, 0xff, 0xff]);
        assert_eq!(&c[12..16], ts.to_be_bytes());
        let second = 16 + SEND_CHUNK;
        assert_eq!(c[second], 0xc0 | 6);
        assert_eq!(&c[second + 1..second + 5], ts.to_be_bytes());
    }

    #[test]
    fn control_messages_use_chunk_stream_2() {
        assert_eq!(
            set_chunk_size(4096),
            [2, 0, 0, 0, 0, 0, 4, 1, 0, 0, 0, 0, 0, 0, 0x10, 0]
        );
        assert_eq!(ping_response(&[0, 6, 1, 2, 3, 4])[12..], [0, 7, 1, 2, 3, 4]);
    }

    #[tokio::test]
    async fn what_we_send_can_be_read_back_across_chunks() {
        let body: Vec<u8> = (0..(SEND_CHUNK * 2 + 33)).map(|i| i as u8).collect();
        let mut wire = set_chunk_size(SEND_CHUNK as u32);
        wire.extend(chunks(6, 0x0100_0000, 9, 1, &body));
        wire.extend(chunks(4, 5, 8, 1, &[9, 9]));
        let mut r = Reader::new();
        let mut s = &wire[..];
        assert_eq!(r.next(&mut s).await.unwrap().kind, MSG_SET_CHUNK_SIZE);
        let m = r.next(&mut s).await.unwrap();
        assert_eq!((m.kind, m.body), (9, body));
        let m = r.next(&mut s).await.unwrap();
        assert_eq!((m.kind, m.body), (8, vec![9, 9]));
        assert!(r.next(&mut s).await.is_err());
    }
}
