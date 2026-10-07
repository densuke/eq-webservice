//! 書き終えた大きなファイルのページキャッシュを捨てる (Issue #196)。
//! cgroup のメモリ (memory.current) にはページキャッシュも数えられる。配信の ring (1 分ぶん数 MB〜20MB 台のファイルを
//! 20 個で回す) と録画の切り出しが書いたぶんは、読み返さない限り要らないのに MemoryMax まで溜まり、
//! 上限に当たるたびの回収待ちが配信の遅れ (busy の札の psi_io・write) に効いていた疑いがある。
//! 書き終えた範囲を捨てれば、ring が常に抱える最大 約 20 分ぶんのキャッシュ (n2 の実測で file 188MB) が、
//! 書いている最中の 1 ファイル分に収まる見込み (効果は適用後の memwatch のログ file_mb で確かめる)。
//! 捨てても中身はディスクにあるので、読み返す切り出しは (ディスクから読むぶん少し遅いだけで) 動作は変わらない。
//! Linux 以外 (macOS) では何もしない。

use std::path::Path;

/// 開いているファイル全体のページキャッシュを捨てる。ディスクに書けていないページは捨てられないので、先に sync_data する。
/// ブロックするので、tokio の中では spawn_blocking で呼ぶこと
pub fn drop_cache(file: &std::fs::File) {
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        if file.sync_data().is_ok() {
            // SAFETY: fd は file が開いている間は有効。範囲 (0, 0) はファイル全体。失敗しても配信には影響しない
            unsafe {
                libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = file;
}

/// パスのファイルのページキャッシュを捨てる (開けなければ何もしない)
pub fn drop_path(path: &Path) {
    if let Ok(f) = std::fs::File::open(path) {
        drop_cache(&f);
    }
}

/// 書き終えた tokio のファイルのページキャッシュを、配信を止めずに (別スレッドで) 捨てる
pub fn drop_cache_in_background(file: tokio::fs::File) {
    tokio::spawn(async move {
        let std = file.into_std().await;
        let _ = tokio::task::spawn_blocking(move || drop_cache(&std)).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn dropping_the_cache_keeps_the_contents() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.ts");
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(&[7u8; 100_000]).unwrap();
        drop_cache(&f);
        drop_path(&p);
        drop_path(&d.path().join("missing"));
        let mut got = Vec::new();
        std::fs::File::open(&p).unwrap().read_to_end(&mut got).unwrap();
        assert_eq!(got, vec![7u8; 100_000]);
    }
}
