//! 録画のリングバッファ (docs/quake-archive.md 3.6 章)。圧縮した ffmpeg の出力 (mpegts) を受け取り、
//! 1 分ごとのファイル `ring/%02d.ts` (20 個で回る) に書く。ffmpeg の tee / segment は使わない。
//! 書き込みは別のタスクで、詰まったら捨てる。失敗はログに出すだけで、送り出しは止めない。

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

/// 1 つのファイルの長さ (秒)。record.rs は、ファイルの更新時刻の前のこの長さをそのファイルの中身とみなす
pub const SEGMENT_SECS: u64 = 60;
/// ring のファイルの数 (00.ts 〜 19.ts)
pub const RING_FILES: usize = 20;
/// 書き込みのタスクに渡せる塊の数。超えたら捨てる (1 塊は数 KB〜数十 KB)
const QUEUE: usize = 256;
const PACKET: usize = 188;

/// 切り分けた 1 片。`new_segment` なら、ここから新しいファイル
#[derive(Debug, PartialEq, Eq)]
struct Piece {
    new_segment: bool,
    data: Vec<u8>,
}

/// mpegts を、キーフレームの位置で `SEGMENT_SECS` ごとのファイルに切り分ける。
/// 新しいファイルの先頭には、直前の PAT・PMT を付けて、そのファイルだけで読めるようにする
#[derive(Default)]
struct Splitter {
    /// 188 バイトに満たない残り
    carry: Vec<u8>,
    pat: Option<Vec<u8>>,
    pmt: Option<Vec<u8>>,
    pmt_pid: Option<u16>,
    video_pid: Option<u16>,
    /// 今のファイルを始めた時刻 (配信の開始からの経過)
    seg_start: Option<Duration>,
}

impl Splitter {
    fn push(&mut self, data: &[u8], now: Duration) -> Vec<Piece> {
        self.carry.extend_from_slice(data);
        let buf = std::mem::take(&mut self.carry);
        let mut out = Vec::new();
        let mut cur = Piece {
            new_segment: false,
            data: Vec::new(),
        };
        let mut i = 0;
        while i + PACKET <= buf.len() {
            if buf[i] != 0x47 {
                i += 1; // 同期が外れた。次の 0x47 まで捨てる
                continue;
            }
            let pkt = &buf[i..i + PACKET];
            i += PACKET;
            let due = self
                .seg_start
                .is_none_or(|s| now.saturating_sub(s) >= Duration::from_secs(SEGMENT_SECS))
                && (self.seg_start.is_none() || self.is_key(pkt));
            if due {
                if !cur.data.is_empty() {
                    out.push(std::mem::replace(
                        &mut cur,
                        Piece {
                            new_segment: false,
                            data: Vec::new(),
                        },
                    ));
                }
                cur.new_segment = true;
                self.seg_start = Some(now);
                for t in [&self.pat, &self.pmt].into_iter().flatten() {
                    cur.data.extend_from_slice(t);
                }
            }
            self.note(pkt);
            cur.data.extend_from_slice(pkt);
        }
        self.carry = buf[i..].to_vec();
        if !cur.data.is_empty() {
            out.push(cur);
        }
        out
    }

    /// PAT・PMT を覚え、PMT から映像の PID を知る
    fn note(&mut self, p: &[u8]) {
        let pid = pid(p);
        if p[1] & 0x40 == 0 {
            return;
        }
        let Some(body) = payload(p).and_then(|b| b.get(1 + usize::from(*b.first()?)..)) else {
            return;
        };
        if pid == 0 && body.first() == Some(&0) {
            // 最初の番組 (program_number が 0 でないもの) の PMT の PID
            self.pmt_pid = body
                .get(8..)
                .and_then(|e| e.as_chunks::<4>().0.iter().find(|c| c[..2] != [0, 0]))
                .map(|c| u16::from(c[2] & 0x1f) << 8 | u16::from(c[3]));
            self.pat = Some(p.to_vec());
        } else if Some(pid) == self.pmt_pid && body.first() == Some(&2) {
            self.video_pid = video_pid(body);
            self.pmt = Some(p.to_vec());
        }
    }

    /// 映像のキーフレームの先頭のパケットか (PUSI と random_access_indicator)
    fn is_key(&self, p: &[u8]) -> bool {
        Some(pid(p)) == self.video_pid && p[1] & 0x40 != 0 && p[3] & 0x20 != 0 && p[4] > 0 && p[5] & 0x40 != 0
    }
}

fn pid(p: &[u8]) -> u16 {
    u16::from(p[1] & 0x1f) << 8 | u16::from(p[2])
}

/// パケットの中身 (adaptation field の後)
fn payload(p: &[u8]) -> Option<&[u8]> {
    let af = p[3] >> 4 & 3;
    if af & 1 == 0 {
        return None;
    }
    p.get(if af & 2 != 0 { 5 + p[4] as usize } else { 4 }..)
}

/// PMT (table_id から始まる) から、H.264 / H.265 の PID
fn video_pid(t: &[u8]) -> Option<u16> {
    let end = (3 + (usize::from(t.get(1)? & 0x0f) << 8 | usize::from(*t.get(2)?))).checked_sub(4)?;
    let mut i = 12 + (usize::from(t.get(10)? & 0x0f) << 8 | usize::from(*t.get(11)?));
    while i + 5 <= end.min(t.len()) {
        if matches!(t[i], 0x1b | 0x24) {
            return Some(u16::from(t[i + 1] & 0x1f) << 8 | u16::from(t[i + 2]));
        }
        i += 5 + (usize::from(t[i + 3] & 0x0f) << 8 | usize::from(t[i + 4]));
    }
    None
}

/// 次に書くファイルの番号 (いちばん新しいファイルの次。再起動で新しいものを上書きしない)
fn next_index(files: &[(PathBuf, SystemTime)]) -> usize {
    files
        .iter()
        .filter_map(|(p, m)| {
            Some((
                p.file_stem()?
                    .to_str()?
                    .parse::<usize>()
                    .ok()
                    .filter(|n| *n < RING_FILES)?,
                m,
            ))
        })
        .max_by_key(|(_, m)| **m)
        .map_or(0, |(n, _)| (n + 1) % RING_FILES)
}

async fn open(dir: &Path, index: usize) -> Option<tokio::fs::File> {
    let path = dir.join(format!("{index:02}.ts"));
    let mut r = tokio::fs::File::create(&path).await;
    if r.is_err() {
        // ディレクトリが消えたときは作り直す
        let _ = tokio::fs::create_dir_all(dir).await;
        r = tokio::fs::File::create(&path).await;
    }
    r.map_err(|e| tracing::warn!("record: {} を開けません: {e}", path.display()))
        .ok()
}

/// 書き込みのタスクを起動し、mpegts を渡す口を返す。口がいっぱいなら try_send が失敗する (捨てる)。
/// 口が閉じられる (送り手が消える) と、タスクは終わる
pub fn spawn(dir: PathBuf) -> mpsc::Sender<Vec<u8>> {
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(QUEUE);
    tokio::spawn(async move {
        let mut index = crate::broadcast::record::list_ring(&dir).map_or(0, |f| next_index(&f));
        let (mut splitter, mut file, t0) = (Splitter::default(), None, std::time::Instant::now());
        while let Some(chunk) = rx.recv().await {
            for p in splitter.push(&chunk, t0.elapsed()) {
                if p.new_segment {
                    file = open(&dir, index).await;
                    index = (index + 1) % RING_FILES;
                }
                if let Some(f) = file.as_mut() {
                    if let Err(e) = f.write_all(&p.data).await {
                        tracing::warn!("record: ring に書けません: {e}");
                        file = None; // 次のファイルで開き直す
                    }
                }
            }
            if let Some(f) = file.as_mut() {
                let _ = f.flush().await;
            }
        }
    });
    tx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(pid: u16, pusi: bool, rai: bool, payload: &[u8]) -> Vec<u8> {
        let mut p = vec![0x47, (pid >> 8) as u8 | if pusi { 0x40 } else { 0 }, pid as u8];
        if rai {
            p.extend([0x30, 7, 0x40]);
            p.resize(4 + 1 + 7, 0xff);
        } else {
            p.push(0x10);
        }
        p.extend_from_slice(payload);
        p.resize(PACKET, 0xff);
        p
    }

    fn pat() -> Vec<u8> {
        // pointer, table_id 0, length 13, tsid, ver, sec, last, program 1 -> PMT 0x1000, CRC
        ts(
            0,
            true,
            false,
            &[0, 0, 0xb0, 13, 0, 1, 0xc1, 0, 0, 0, 1, 0xf0, 0, 0, 0, 0, 0],
        )
    }

    fn pmt() -> Vec<u8> {
        // table_id 2, length 18: 5 (head) + 2 (program_info_length=0 の 2 バイトぶんを含む) ... H.264 0x100, AAC 0x101
        let t = [
            0, 2, 0xb0, 23, 0, 1, 0xc1, 0, 0, 0xe1, 0, 0xf0, 0, 0x1b, 0xe1, 0, 0xf0, 0, 0x0f, 0xe1, 1, 0xf0, 0, 0, 0,
            0, 0,
        ];
        ts(0x1000, true, false, &t)
    }

    /// PAT・PMT の後に、映像 (keyframe か) 1 パケットを並べたもの
    fn unit(key: bool) -> Vec<u8> {
        [pat(), pmt(), ts(0x100, true, key, &[0, 0, 1])].concat()
    }

    #[test]
    fn the_file_changes_at_the_first_keyframe_after_a_minute_and_starts_with_pat_and_pmt() {
        let mut s = Splitter::default();
        let a = s.push(&unit(true), Duration::ZERO);
        assert_eq!(a.len(), 1);
        assert!(a[0].new_segment);
        // 59 秒: キーフレームでも切らない
        assert!(s
            .push(&unit(true), Duration::from_secs(59))
            .iter()
            .all(|p| !p.new_segment));
        // 61 秒でも、キーフレームでなければ切らない (PAT・PMT だけのところでも切らない)
        let b = s.push(&unit(false), Duration::from_secs(61));
        assert!(b.iter().all(|p| !p.new_segment));
        // キーフレームで切る。新しいファイルは PAT・PMT から始まる
        let c = s.push(&ts(0x100, true, true, &[0, 0, 1]), Duration::from_secs(62));
        assert_eq!(c.len(), 1);
        assert!(c[0].new_segment);
        assert_eq!(&c[0].data[..PACKET], &pat()[..]);
        assert_eq!(&c[0].data[PACKET..2 * PACKET], &pmt()[..]);
        assert_eq!(c[0].data.len(), 3 * PACKET);
        // 次は、そこからまた 60 秒後
        assert!(s
            .push(&unit(true), Duration::from_secs(100))
            .iter()
            .all(|p| !p.new_segment));
        assert!(s
            .push(&unit(true), Duration::from_secs(122))
            .iter()
            .any(|p| p.new_segment));
    }

    #[test]
    fn packets_split_across_chunks_are_not_lost() {
        let mut s = Splitter::default();
        let all = unit(true);
        let got: usize = all
            .chunks(100)
            .map(|c| s.push(c, Duration::ZERO).iter().map(|p| p.data.len()).sum::<usize>())
            .sum();
        assert_eq!(got, all.len());
    }

    #[test]
    fn the_video_pid_is_read_from_the_pmt() {
        let mut s = Splitter::default();
        s.push(&unit(false), Duration::ZERO);
        assert_eq!(s.pmt_pid, Some(0x1000));
        assert_eq!(s.video_pid, Some(0x100));
    }

    #[test]
    fn the_next_file_follows_the_newest_one() {
        let at = |s| SystemTime::UNIX_EPOCH + Duration::from_secs(s);
        let f = |n: &str, s| (PathBuf::from(n), at(s));
        assert_eq!(next_index(&[]), 0);
        assert_eq!(
            next_index(&[f("00.ts", 10), f("03.ts", 30), f("02.ts", 20), f("x.txt", 99)]),
            4
        );
        assert_eq!(next_index(&[f("19.ts", 50), f("00.ts", 10)]), 0);
    }

    #[tokio::test]
    async fn files_are_written_one_per_minute_and_wrap_at_twenty() {
        // 21 分ぶんを流すと、00〜19 の 20 個になり、21 個目は 00 に戻る (時刻は Splitter に直接渡す)
        let dir = tempfile::tempdir().unwrap();
        let mut s = Splitter::default();
        let mut index = 0;
        for min in 0..21u64 {
            for p in s.push(&unit(true), Duration::from_secs(min * 60 + 1)) {
                if p.new_segment {
                    let mut f = open(dir.path(), index).await.unwrap();
                    f.write_all(&p.data).await.unwrap();
                    index = (index + 1) % RING_FILES;
                }
            }
        }
        let mut names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names.len(), 20);
        assert_eq!(names[0], "00.ts");
        assert_eq!(names[19], "19.ts");
        assert_eq!(index, 1);
    }

    #[tokio::test]
    async fn a_missing_directory_or_a_full_queue_never_blocks_the_sender() {
        let dir = tempfile::tempdir().unwrap();
        let ring = dir.path().join("gone/ring");
        let tx = spawn(ring.clone());
        // 口がいっぱいでも try_send は待たずに失敗するだけ (送り出しを止めない)
        let dropped = (0..QUEUE * 4).filter(|_| tx.try_send(unit(true)).is_err()).count();
        assert!(dropped > 0);
        tokio::time::sleep(Duration::from_millis(200)).await;
        // ディレクトリが無くても作り直して書く
        assert!(ring.join("00.ts").exists());
    }
}
