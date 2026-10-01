//! 動画を作る 1 回分の起動 (systemd の一時的なユーザー単位)。配信より先に譲る仕事なので、CPU・メモリ・優先度を縛る。
//! 引数と名前を作るだけの純粋な関数。起動と止める操作は mod.rs。

use std::path::{Path, PathBuf};

use super::config::{expand, WorkerConfig};
use super::detect::Quake;
use super::queue::Job;

/// 単位の名前。やり直しのたびに変える (前の単位が片づく前でも、名前がぶつからないように)
pub fn unit_name(id: &str, now_secs: u64) -> String {
    format!("eq-replay-job-{id}-{now_secs}")
}

/// 作りかけの動画と、チャプターの JSON (work/ の下)
pub fn work_files(cfg: &WorkerConfig, id: &str) -> (PathBuf, PathBuf) {
    let dir = cfg.work_dir();
    (dir.join(format!("{id}.mp4")), dir.join(format!("{id}.chapters.json")))
}

/// replay-video の `--quake` の値
pub fn quake_arg(q: &Quake) -> String {
    match (q.lat, q.lon) {
        (Some(lat), Some(lon)) => format!("{},{lat},{lon}", q.origin_ms),
        _ => q.origin_ms.to_string(),
    }
}

/// `systemd-run` の引数: --wait で終わるまで待ち、--collect で失敗した単位も片づける。
/// 作業ディレクトリは作る係と同じにする (map_dir などの相対パスが、そのまま通る)
pub fn systemd_run_args(
    cfg: &WorkerConfig,
    job: &Job,
    unit: &str,
    exe: &Path,
    cwd: &Path,
    out: &Path,
    chapters: &Path,
) -> Vec<String> {
    let s = |v: &str| v.to_string();
    let p = |v: &Path| v.display().to_string();
    let mut a = vec![
        s("--user"),
        s("--wait"),
        s("--collect"),
        s("--quiet"),
        format!("--unit={unit}"),
        s("-p"),
        format!("CPUQuota={}", cfg.cpu_quota),
        s("-p"),
        s("CPUWeight=1"),
        s("-p"),
        format!("MemoryMax={}", cfg.memory_max),
        s("-p"),
        s("MemorySwapMax=0"),
        s("-p"),
        s("Nice=19"),
        s("-p"),
        format!("WorkingDirectory={}", p(cwd)),
        p(exe),
        s("replay-video"),
        s("--from"),
        job.from_ms.to_string(),
        s("--to"),
        job.to_ms.to_string(),
        s("--out"),
        p(out),
        s("--events"),
        p(&cfg.events_path()),
        s("--chapters"),
        p(chapters),
        s("--fps"),
        cfg.fps.to_string(),
        s("--label"),
        cfg.label.clone(),
        s("--map-dir"),
        p(&expand(&cfg.map_dir)),
        s("--font"),
        p(&expand(&cfg.font)),
        s("--ffmpeg"),
        cfg.ffmpeg.clone(),
    ];
    for q in &job.quakes {
        a.extend([s("--quake"), quake_arg(q)]);
    }
    a
}

#[cfg(test)]
mod tests {
    use super::super::detect::{groups, Rules};
    use super::*;

    fn job() -> Job {
        let q = |origin_ms, at: Option<(f64, f64)>| Quake {
            origin_ms,
            lat: at.map(|a| a.0),
            lon: at.map(|a| a.1),
            max_scale: 30,
            warning: false,
            name: String::new(),
            last_recv_ms: origin_ms as u64,
        };
        let rules = Rules::from(&WorkerConfig::default());
        let g = groups(&[q(1_000_000, Some((35.5, 139.5))), q(1_060_000, None)], &rules);
        Job::new(&g[0], 5)
    }

    #[test]
    fn the_unit_name_changes_with_each_attempt() {
        assert_ne!(unit_name("20260930-140003", 1), unit_name("20260930-140003", 2));
        assert_eq!(unit_name("a", 7), "eq-replay-job-a-7");
    }

    #[test]
    fn a_quake_argument_carries_the_epicenter_only_when_known() {
        let j = job();
        assert_eq!(quake_arg(&j.quakes[0]), "1000000,35.5,139.5");
        assert_eq!(quake_arg(&j.quakes[1]), "1060000");
    }

    #[test]
    fn the_job_runs_niced_and_capped_and_waits_for_the_unit() {
        let cfg = WorkerConfig::default();
        let j = job();
        let a = systemd_run_args(
            &cfg,
            &j,
            "eq-replay-job-x-1",
            Path::new("/opt/eq-server"),
            Path::new("/srv/cast"),
            Path::new("/w/x.mp4"),
            Path::new("/w/x.chapters.json"),
        );
        let has = |pair: [&str; 2]| a.windows(2).any(|w| w == pair);
        assert!(a.contains(&"--wait".to_string()) && a.contains(&"--collect".to_string()));
        assert!(a.contains(&"--unit=eq-replay-job-x-1".to_string()));
        for prop in [
            "CPUQuota=5%",
            "CPUWeight=1",
            "MemoryMax=200M",
            "MemorySwapMax=0",
            "Nice=19",
            "WorkingDirectory=/srv/cast",
        ] {
            assert!(has(["-p", prop]), "{prop} is missing: {a:?}");
        }
        // 単位の引数のあとが、replay-video
        let exe = a.iter().position(|x| x == "/opt/eq-server").unwrap();
        assert_eq!(a[exe + 1], "replay-video");
        assert!(has(["--from", &j.from_ms.to_string()]) && has(["--to", &j.to_ms.to_string()]));
        assert!(has(["--out", "/w/x.mp4"]) && has(["--chapters", "/w/x.chapters.json"]));
        assert!(has(["--quake", "1000000,35.5,139.5"]) && has(["--quake", "1060000"]));
        // 作る係の systemd-run の引数より後ろに、replay-video の引数が来る (単位の設定が replay-video に渡らない)
        assert!(a.iter().position(|x| x == "--unit=eq-replay-job-x-1").unwrap() < exe);
    }

    #[test]
    fn the_work_files_live_under_work() {
        let cfg = WorkerConfig {
            dir: "/srv/r".into(),
            ..Default::default()
        };
        let (out, chapters) = work_files(&cfg, "id1");
        assert_eq!(out, PathBuf::from("/srv/r/work/id1.mp4"));
        assert_eq!(chapters, PathBuf::from("/srv/r/work/id1.chapters.json"));
    }
}
