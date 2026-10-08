//! 動画を作る 1 回分の起動 (systemd の一時的なユーザー単位)。配信より先に譲る仕事なので、CPU・メモリ・優先度を縛る。
//! 引数と名前を作るだけの純粋な関数。起動と止める操作は mod.rs。

use std::path::{Path, PathBuf};

use super::config::{expand, EventsSource, WorkerConfig};
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

/// replay-video に渡す引数 (サブコマンド名から。runner によらず同じ)
pub fn replay_video_args(cfg: &WorkerConfig, job: &Job, out: &Path, chapters: &Path) -> Vec<String> {
    let s = |v: &str| v.to_string();
    let p = |v: &Path| v.display().to_string();
    let mut a = vec![
        s("replay-video"),
        s("--from"),
        job.from_ms.to_string(),
        s("--to"),
        job.to_ms.to_string(),
        s("--out"),
        p(out),
    ];
    a.extend(match cfg.events_source() {
        EventsSource::File(path) => [s("--events"), p(&path)],
        EventsSource::Url(url) => [s("--archive"), url],
    });
    a.extend([
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
    ]);
    if !cfg.voice_server.is_empty() {
        a.extend([s("--voice"), cfg.voice_server.clone()]);
    }
    for q in &job.quakes {
        a.extend([s("--quake"), quake_arg(q)]);
    }
    a
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
        // I/O も最低 (ionice -c3 相当)。単位の設定なので、中の ffmpeg にも効く
        s("-p"),
        s("IOSchedulingClass=idle"),
        s("-p"),
        format!("WorkingDirectory={}", p(cwd)),
        p(exe),
    ];
    a.extend(replay_video_args(cfg, job, out, chapters));
    a
}

/// inline で子の前に付ける優先度を下げるコマンドの既定 (`inline_wrap` が空のとき)。
/// macOS は background 扱い (CPU・ディスク)、他は nice 19 (I/O までは下げられない)
pub fn default_inline_wrap(os: &str) -> Vec<String> {
    let v: &[&str] = if os == "macos" {
        &["/usr/sbin/taskpolicy", "-b"]
    } else {
        &["nice", "-n", "19"]
    };
    v.iter().map(|x| x.to_string()).collect()
}

/// inline で起動するコマンド全体 (先頭が実行ファイル)。`inline_wrap` (優先度を下げるコマンド。空なら OS の既定) の後ろに、
/// eq-server と replay-video の引数を続ける。作業ディレクトリは作る係のまま (子は継ぐ)
pub fn inline_command(cfg: &WorkerConfig, job: &Job, exe: &Path, out: &Path, chapters: &Path) -> Vec<String> {
    let mut a = if cfg.inline_wrap.is_empty() {
        default_inline_wrap(std::env::consts::OS)
    } else {
        cfg.inline_wrap.clone()
    };
    a.push(exe.display().to_string());
    a.extend(replay_video_args(cfg, job, out, chapters));
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
            magnitude: None,
            depth_km: None,
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
            "IOSchedulingClass=idle",
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
    fn the_inline_command_has_the_same_replay_video_arguments_as_systemd() {
        let cfg = WorkerConfig {
            inline_wrap: vec!["/usr/sbin/taskpolicy".into(), "-b".into()],
            ..Default::default()
        };
        let j = job();
        let (exe, out, ch) = (
            Path::new("/opt/eq-server"),
            Path::new("/w/x.mp4"),
            Path::new("/w/x.chapters.json"),
        );
        let sys = systemd_run_args(&cfg, &j, "u", exe, Path::new("/srv"), out, ch);
        let inline = inline_command(&cfg, &j, exe, out, ch);
        // systemd-run の単位の設定を除くと、eq-server 以降は同じ
        let tail = |a: &[String]| a[a.iter().position(|x| x == "/opt/eq-server").unwrap()..].to_vec();
        assert_eq!(tail(&sys), tail(&inline));
        assert_eq!(&inline[..3], ["/usr/sbin/taskpolicy", "-b", "/opt/eq-server"]);
        assert_eq!(inline[3], "replay-video");
        // systemd の縛りは付かない
        assert!(!inline
            .iter()
            .any(|x| x.contains("CPUQuota") || x.contains("MemoryMax") || x == "--wait"));
        // wrap が空なら、OS の既定の優先度コマンドが付く
        let dflt = inline_command(&WorkerConfig::default(), &j, exe, out, ch);
        let n = default_inline_wrap(std::env::consts::OS).len();
        assert_eq!(dflt[..n], default_inline_wrap(std::env::consts::OS));
        assert_eq!(&dflt[n..n + 2], ["/opt/eq-server", "replay-video"]);
    }

    #[test]
    fn the_default_inline_wrap_lowers_priority_per_os() {
        assert_eq!(default_inline_wrap("macos"), ["/usr/sbin/taskpolicy", "-b"]);
        assert_eq!(default_inline_wrap("linux"), ["nice", "-n", "19"]);
    }

    #[test]
    fn an_archive_url_is_passed_as_archive_not_events() {
        let j = job();
        let args = |events: &str| {
            let cfg = WorkerConfig {
                events: events.into(),
                ..Default::default()
            };
            replay_video_args(&cfg, &j, Path::new("/o.mp4"), Path::new("/c.json"))
        };
        let has = |a: &[String], pair: [&str; 2]| a.windows(2).any(|w| w == pair);
        let url = args("https://eq.fuga.jp/");
        assert!(has(&url, ["--archive", "https://eq.fuga.jp"]) && !url.contains(&"--events".to_string()));
        let file = args("/srv/e.jsonl");
        assert!(has(&file, ["--events", "/srv/e.jsonl"]) && !file.contains(&"--archive".to_string()));
    }

    #[test]
    fn voice_is_passed_only_when_a_server_is_configured() {
        let j = job();
        let args = |voice_server: &str| {
            let cfg = WorkerConfig {
                voice_server: voice_server.into(),
                ..Default::default()
            };
            replay_video_args(&cfg, &j, Path::new("/o.mp4"), Path::new("/c.json"))
        };
        let has = |a: &[String], pair: [&str; 2]| a.windows(2).any(|w| w == pair);
        assert!(!args("").contains(&"--voice".to_string()));
        assert!(has(&args("https://eq.fuga.jp"), ["--voice", "https://eq.fuga.jp"]));
        let parsed: WorkerConfig = toml::from_str("voice_server = \"https://eq.fuga.jp\"").unwrap();
        assert_eq!(parsed.voice_server, "https://eq.fuga.jp");
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
