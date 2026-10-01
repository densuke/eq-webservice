//! 作る係の設定 (`replay.toml` の `[replay]` の節。docs/replay-video.md 5.2 の 3)。省いた項目は既定値になる。

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;

use super::super::super::BroadcastConfig;
use super::hours::Hours;

/// 作っている間に重くなったとき (e2 が詰まった・YouTube の健全性が落ちた) どうするか
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnBusy {
    /// すぐ止めて、作りかけを消す (既定。凍結はメモリを抱えたままで、配信を助けきれない)
    Kill,
    /// 凍結して、落ち着いたら解凍する
    Freeze,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct WorkerConfig {
    // 置き場所
    /// eq-server の記録 (sink の jsonl)
    pub events: String,
    /// 作る係の作業場所。queue/ (待ちの印)・work/ (作りかけ)・done/ (できた動画) を下に作る
    pub dir: String,
    /// 配信の状態のファイル (broadcast の state_file)。空なら `$XDG_STATE_HOME/eq-broadcast/state.json`
    pub state_file: String,

    // 検知
    /// この震度 (10 = 震度 1 ... 30 = 震度 3) 以上の地震を動画にする。録画 (broadcast.toml の [record]) の min_scale と同じ値にする
    pub min_scale: i32,
    /// 緊急地震速報の警報が出た地震は、震度にかかわらず動画にする
    pub warning: bool,

    // まとめ
    /// 前の地震から、この距離 (km) 以内の震源なら、連続地震として 1 本にまとめる
    pub join_km: f64,
    /// 前の地震から、この時間 (分) 以内に起きたら、まとめる
    pub join_min: u64,
    /// 最後の地震の報から、この時間 (分) 静かになったら、まとまりを閉じて動画にする
    pub quiet_min: u64,
    /// 1 本にまとめる長さの上限 (時間)。超えたら、次のまとまりにする
    pub max_group_hours: u64,
    /// 記録を見に行く範囲 (時間)。max_group_hours + quiet_min より長くする
    pub lookback_hours: u64,
    /// 記録を見直す間隔 (秒)
    pub scan_secs: u64,

    // 作る・見張る
    /// 見張る間隔 (秒)
    pub tick_secs: u64,
    /// 最後の地震の画面から、この時間 (分) たつまで、作り始めない
    pub calm_min: u64,
    /// 作り始める時間帯 (日本時間の時。"1-5" は 1:00 から 5:00 まで)。作りかけは終わりを過ぎても続ける
    pub hours: String,
    /// 作り始めるのに必要な、e2 のメモリの空き (MemAvailable。MB)。読めない (Mac) ときは見ない
    pub min_mem_mb: u64,
    /// e2 の PSI (io・memory の full の 60 秒平均。%) がこれを超えたら、重いとみなす
    pub psi_limit: f64,
    /// 重くなったときの動き。"kill" (止めて待ちに戻す。やり直しに数える) か "freeze" (凍結)
    pub on_busy: OnBusy,
    /// 凍結 (on_busy = "freeze") がこの時間 (分) 続いたら、止めて待ちに戻す
    pub freeze_give_up_min: u64,
    /// やり直しがこの回数を超えたら、失敗にする
    pub max_retries: u32,
    /// 失敗してから、やり直すまでの時間 (分)
    pub retry_wait_min: u64,
    /// 作る間の CPU・メモリの上限 (systemd の CPUQuota・MemoryMax)
    pub cpu_quota: String,
    pub memory_max: String,

    // YouTube の健全性 (作っている間だけ見る)
    /// 見る間隔 (秒)
    pub health_secs: u64,
    /// ライブの受け口 (liveStreams) の名前
    pub stream_name: String,
    /// 認証情報 (google-auth の authorized user の JSON。読むだけで書き戻さない)。空なら健全性を見ない
    pub youtube_token: String,

    // replay-video に渡すもの
    pub fps: u32,
    pub label: String,
    pub map_dir: String,
    pub font: String,
    pub ffmpeg: String,

    // 外のコマンド (Linux 以外で、代わりのものに差し替えて確かめるため)
    pub systemd_run: String,
    pub systemctl: String,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        let b = BroadcastConfig::default();
        WorkerConfig {
            events: "data/events.jsonl".into(),
            dir: "~/work/eq-replay".into(),
            state_file: String::new(),
            min_scale: 30,
            warning: true,
            join_km: 50.0,
            join_min: 30,
            quiet_min: 60,
            max_group_hours: 3,
            lookback_hours: 6,
            scan_secs: 300,
            tick_secs: 5,
            calm_min: 30,
            hours: "1-5".into(),
            min_mem_mb: 250,
            psi_limit: 20.0,
            on_busy: OnBusy::Kill,
            freeze_give_up_min: 10,
            max_retries: 5,
            retry_wait_min: 10,
            cpu_quota: "5%".into(),
            memory_max: "200M".into(),
            health_secs: 60,
            stream_name: "地震モニター用".into(),
            youtube_token: "~/.config/pd2/youtube-upload-token.json".into(),
            fps: 5,
            label: "記録から再現".into(),
            map_dir: b.map_dir,
            font: b.font,
            ffmpeg: b.ffmpeg,
            systemd_run: "systemd-run".into(),
            systemctl: "systemctl".into(),
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    replay: WorkerConfig,
}

/// 設定のファイルを読む (無ければ全部既定値。パスを渡して読めないときは失敗)
pub fn load(path: Option<&str>) -> anyhow::Result<WorkerConfig> {
    let Some(path) = path else {
        return Ok(WorkerConfig::default());
    };
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
    let file: File = toml::from_str(&text).with_context(|| format!("parsing {path}"))?;
    file.replay.check()
}

impl WorkerConfig {
    fn check(self) -> anyhow::Result<Self> {
        anyhow::ensure!(
            self.lookback_hours * 60 > self.max_group_hours * 60 + self.quiet_min,
            "lookback_hours は max_group_hours + quiet_min より長くしてください (まとまりを途中で切らないため)"
        );
        anyhow::ensure!(
            self.tick_secs > 0 && self.scan_secs > 0 && self.health_secs > 0,
            "間隔は 1 秒以上"
        );
        anyhow::ensure!((1..=60).contains(&self.fps), "fps must be 1..=60");
        Hours::parse(&self.hours)?;
        Ok(self)
    }

    /// 作り始めてよい時間帯 (`check` で確かめ済み)
    pub fn start_hours(&self) -> Hours {
        Hours::parse(&self.hours).expect("hours is validated by check()")
    }

    /// eq-server の記録 (`~/` はホームディレクトリ)
    pub fn events_path(&self) -> PathBuf {
        expand(&self.events)
    }

    pub fn queue_dir(&self) -> PathBuf {
        expand(&self.dir).join("queue")
    }

    pub fn work_dir(&self) -> PathBuf {
        expand(&self.dir).join("work")
    }

    pub fn done_dir(&self) -> PathBuf {
        expand(&self.dir).join("done")
    }
}

/// 先頭の `~/` を、ホームディレクトリにする
pub fn expand(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest),
        _ => PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_section_is_all_defaults_and_unknown_keys_are_refused() {
        let f: File = toml::from_str("").unwrap();
        let c = f.replay.check().unwrap();
        // 録画 ([record]) の min_scale と同じ 30
        assert_eq!(
            (c.min_scale, c.join_km, c.join_min, c.quiet_min, c.max_group_hours),
            (30, 50.0, 30, 60, 3)
        );
        assert_eq!(
            (c.calm_min, c.psi_limit, c.freeze_give_up_min, c.max_retries),
            (30, 20.0, 10, 5)
        );
        assert_eq!(c.cpu_quota, "5%");
        assert_eq!((c.hours.as_str(), c.min_mem_mb, c.on_busy), ("1-5", 250, OnBusy::Kill));
        let f: File = toml::from_str("[replay]\nmin_scale = 40\ncpu_quota = \"10%\"").unwrap();
        assert_eq!((f.replay.min_scale, f.replay.cpu_quota.as_str()), (40, "10%"));
        assert!(toml::from_str::<File>("[replay]\nnope = 1").is_err());
    }

    #[test]
    fn on_busy_and_hours_are_checked() {
        let f: File = toml::from_str("[replay]\non_busy = \"freeze\"\nhours = \"22-5\"").unwrap();
        assert_eq!(f.replay.check().unwrap().on_busy, OnBusy::Freeze);
        assert!(toml::from_str::<File>("[replay]\non_busy = \"pause\"").is_err());
        let f: File = toml::from_str("[replay]\nhours = \"9\"").unwrap();
        assert!(f.replay.check().is_err());
    }

    #[test]
    fn the_deploy_example_parses_and_states_the_defaults() {
        let f: File = toml::from_str(include_str!("../../../../../../deploy/replay.e2.toml")).unwrap();
        let c = f.replay.check().unwrap();
        let d = WorkerConfig::default();
        assert_eq!(
            (c.min_scale, c.join_km, c.join_min, c.quiet_min),
            (d.min_scale, d.join_km, d.join_min, d.quiet_min)
        );
        assert_eq!(
            (c.calm_min, c.psi_limit, c.cpu_quota.as_str()),
            (d.calm_min, d.psi_limit, d.cpu_quota.as_str())
        );
        assert_eq!(
            (c.hours.as_str(), c.min_mem_mb, c.on_busy),
            (d.hours.as_str(), d.min_mem_mb, d.on_busy)
        );
        assert_eq!(
            (c.max_group_hours, c.lookback_hours, c.max_retries),
            (d.max_group_hours, d.lookback_hours, d.max_retries)
        );
    }

    #[test]
    fn the_lookback_must_cover_a_whole_group() {
        let f: File = toml::from_str("[replay]\nlookback_hours = 4").unwrap();
        assert!(f.replay.check().is_err());
    }

    #[test]
    fn a_tilde_is_the_home_directory() {
        assert_eq!(expand("/a/b"), PathBuf::from("/a/b"));
        if let Some(home) = std::env::var_os("HOME") {
            assert_eq!(expand("~/x"), Path::new(&home).join("x"));
            let c = WorkerConfig {
                events: "~/data/e.jsonl".into(),
                ..Default::default()
            };
            assert_eq!(c.events_path(), Path::new(&home).join("data/e.jsonl"));
        }
        let c = WorkerConfig {
            dir: "/srv/r".into(),
            ..Default::default()
        };
        assert_eq!(c.queue_dir(), PathBuf::from("/srv/r/queue"));
        assert_eq!(c.done_dir(), PathBuf::from("/srv/r/done"));
        assert_eq!(c.work_dir(), PathBuf::from("/srv/r/work"));
    }
}
