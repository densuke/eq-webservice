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

/// 動画を作る子 (replay-video) の起動のしかた
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Runner {
    /// systemd-run のユーザー単位 (既定。e2。CPU・メモリを縛り、凍結もできる)
    Systemd,
    /// 子プロセスとして直接起動する (Mac)。縛りも凍結も無い
    Inline,
}

/// 作り始める・止める判断に、ライブの配信の様子を使うか
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gate {
    /// 配信の状態のファイル・PSI・メモリの空き・YouTube の健全性を見る (既定。e2 は配信と同じ機械)
    Vm,
    /// 見ない。時間帯 (hours) だけが効く (配信と別の機械の Mac)
    None,
}

/// 記録の読み先
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventsSource {
    File(PathBuf),
    /// eq-server の URL (/api/archive から取る)
    Url(String),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct WorkerConfig {
    // 置き場所
    /// eq-server の記録。sink の jsonl のパスか、`https://eq.fuga.jp` のような URL (/api/archive から取る)
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
    /// events が URL のとき、止まっていた間 (スリープ・オフライン) をさかのぼって見る上限 (時間)。
    /// 前に見終えた時刻 (checkpoint) からさかのぼり、これより古い分は見ない。checkpoint が無い最初は lookback_hours
    pub catchup_max_hours: u64,

    // 作る・見張る
    /// 見張る間隔 (秒)
    pub tick_secs: u64,
    /// 最後の地震の画面から、この時間 (分) たつまで、作り始めない
    pub calm_min: u64,
    /// 作り始める時間帯 (日本時間。"1-5" は 1:00 から 5:00 まで、"10-15,21:30-23" のように複数・分も書ける)。作りかけは終わりを過ぎても続ける
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
    /// 子の起動のしかた。"systemd" (既定) か "inline"
    pub runner: Runner,
    /// 配信の様子を見るか。"vm" (既定) か "none"
    pub gate: Gate,
    /// runner = "inline" のとき、子の前に付けるコマンド (優先度を最低にするため。Mac は ["/usr/sbin/taskpolicy", "-b"])
    pub inline_wrap: Vec<String>,
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

    // YouTube への投稿 (docs/replay-video.md 5.3・6.8)。トークンのファイルが空なら、上げない
    /// youtube-auth で作ったトークンのファイル。空なら投稿しない
    pub youtube_upload_token: String,
    /// Google Cloud の OAuth クライアントの JSON (デスクトップアプリ)。トークンを使うときは要る
    pub youtube_client: String,
    /// 公開範囲。"private" (既定)・"unlisted"・"public"。公開への変更は、見てから YouTube Studio で手で
    pub youtube_privacy: String,
    /// カテゴリ ID (25 = ニュースと政治)
    pub youtube_category: String,
    /// 1 日 (太平洋時間) に上げる本数の上限
    pub youtube_daily_limit: u32,
    /// 上げたあと、done/ の mp4 を消すか
    pub youtube_delete_after_upload: bool,

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
            catchup_max_hours: 24,
            tick_secs: 5,
            calm_min: 30,
            hours: "10-15,21:30-23".into(),
            min_mem_mb: 250,
            psi_limit: 20.0,
            on_busy: OnBusy::Kill,
            freeze_give_up_min: 10,
            max_retries: 5,
            retry_wait_min: 10,
            runner: Runner::Systemd,
            gate: Gate::Vm,
            inline_wrap: Vec::new(),
            cpu_quota: "5%".into(),
            memory_max: "200M".into(),
            health_secs: 60,
            stream_name: "地震モニター用".into(),
            youtube_token: "~/.config/pd2/youtube-upload-token.json".into(),
            youtube_upload_token: String::new(),
            youtube_client: String::new(),
            youtube_privacy: "private".into(),
            youtube_category: "25".into(),
            youtube_daily_limit: 3,
            youtube_delete_after_upload: false,
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
        anyhow::ensure!(
            self.catchup_max_hours >= self.lookback_hours,
            "catchup_max_hours は lookback_hours 以上にしてください"
        );
        anyhow::ensure!(
            !(self.runner == Runner::Inline && self.gate == Gate::Vm && self.on_busy == OnBusy::Freeze),
            "凍結 (on_busy = \"freeze\") は runner = \"systemd\" でだけ使えます"
        );
        Hours::parse(&self.hours)?;
        anyhow::ensure!(
            ["private", "unlisted", "public"].contains(&self.youtube_privacy.as_str()),
            "youtube_privacy は \"private\"・\"unlisted\"・\"public\" のどれか"
        );
        anyhow::ensure!(
            self.youtube_upload_token.is_empty() || !self.youtube_client.is_empty(),
            "youtube_upload_token を使うときは、youtube_client も書いてください"
        );
        Ok(self)
    }

    /// 作り始めてよい時間帯 (`check` で確かめ済み)
    pub fn start_hours(&self) -> Hours {
        Hours::parse(&self.hours).expect("hours is validated by check()")
    }

    /// eq-server の記録の読み先。`http://` か `https://` で始まれば URL (末尾の `/` は除く)
    pub fn events_source(&self) -> EventsSource {
        if self.events.starts_with("http://") || self.events.starts_with("https://") {
            EventsSource::Url(self.events.trim_end_matches('/').to_string())
        } else {
            EventsSource::File(expand(&self.events))
        }
    }

    /// 前に見終えた時刻の印 (checkpoint)
    pub fn checkpoint_path(&self) -> PathBuf {
        expand(&self.dir).join("checkpoint.json")
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
        assert_eq!(
            (c.hours.as_str(), c.min_mem_mb, c.on_busy),
            ("10-15,21:30-23", 250, OnBusy::Kill)
        );
        let f: File = toml::from_str("[replay]\nmin_scale = 40\ncpu_quota = \"10%\"").unwrap();
        assert_eq!((f.replay.min_scale, f.replay.cpu_quota.as_str()), (40, "10%"));
        assert!(toml::from_str::<File>("[replay]\nnope = 1").is_err());
    }

    #[test]
    fn youtube_upload_is_off_and_private_by_default() {
        let c = WorkerConfig::default();
        assert_eq!(
            (
                c.youtube_upload_token.as_str(),
                c.youtube_privacy.as_str(),
                c.youtube_category.as_str(),
                c.youtube_daily_limit,
                c.youtube_delete_after_upload
            ),
            ("", "private", "25", 3, false)
        );
    }

    #[test]
    fn youtube_settings_are_checked() {
        let ok = |t: &str| toml::from_str::<File>(t).unwrap().replay.check();
        assert!(ok("[replay]\nyoutube_privacy = \"unlisted\"").is_ok());
        assert!(ok("[replay]\nyoutube_privacy = \"public \"").is_err());
        assert!(ok("[replay]\nyoutube_privacy = \"secret\"").is_err());
        // トークンを使うなら、クライアントも要る
        assert!(ok("[replay]\nyoutube_upload_token = \"~/t.json\"").is_err());
        assert!(ok("[replay]\nyoutube_upload_token = \"~/t.json\"\nyoutube_client = \"~/c.json\"").is_ok());
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
    fn the_runner_and_the_gate_default_to_the_vm_behaviour() {
        let c = File::default().replay.check().unwrap();
        assert_eq!((c.runner, c.gate, c.catchup_max_hours), (Runner::Systemd, Gate::Vm, 24));
        assert!(c.inline_wrap.is_empty());
        let f: File =
            toml::from_str("[replay]\nrunner = \"inline\"\ngate = \"none\"\ninline_wrap = [\"nice\", \"-n19\"]")
                .unwrap();
        let c = f.replay.check().unwrap();
        assert_eq!((c.runner, c.gate), (Runner::Inline, Gate::None));
        assert_eq!(c.inline_wrap, ["nice", "-n19"]);
        assert!(toml::from_str::<File>("[replay]\nrunner = \"docker\"").is_err());
        assert!(toml::from_str::<File>("[replay]\ngate = \"off\"").is_err());
    }

    #[test]
    fn freezing_needs_systemd_and_the_catchup_covers_the_lookback() {
        let bad = |t: &str| toml::from_str::<File>(t).unwrap().replay.check().is_err();
        assert!(bad("[replay]\nrunner = \"inline\"\non_busy = \"freeze\""));
        assert!(!bad(
            "[replay]\nrunner = \"inline\"\ngate = \"none\"\non_busy = \"freeze\""
        ));
        assert!(bad("[replay]\ncatchup_max_hours = 5"));
    }

    #[test]
    fn the_events_setting_is_a_file_or_an_archive_url() {
        let src = |e: &str| {
            WorkerConfig {
                events: e.into(),
                ..Default::default()
            }
            .events_source()
        };
        assert_eq!(
            src("https://eq.fuga.jp/"),
            EventsSource::Url("https://eq.fuga.jp".into())
        );
        assert_eq!(
            src("http://localhost:9995"),
            EventsSource::Url("http://localhost:9995".into())
        );
        assert_eq!(src("/srv/e.jsonl"), EventsSource::File(PathBuf::from("/srv/e.jsonl")));
        assert_eq!(
            src("data/events.jsonl"),
            EventsSource::File(PathBuf::from("data/events.jsonl"))
        );
    }

    #[test]
    fn the_mac_example_runs_inline_without_the_vm_gate() {
        let f: File = toml::from_str(include_str!("../../../../../../deploy/replay.mac.toml")).unwrap();
        let c = f.replay.check().unwrap();
        assert_eq!((c.runner, c.gate), (Runner::Inline, Gate::None));
        assert_eq!(c.events_source(), EventsSource::Url("https://eq.fuga.jp".into()));
        assert_eq!((c.fps, c.hours.as_str(), c.catchup_max_hours), (15, "0-24", 168));
        assert_eq!(c.inline_wrap, ["/usr/sbin/taskpolicy", "-b"]);
        assert!(c.youtube_token.is_empty());
        // 投稿は、トークンのファイルがあれば動く。公開範囲は非公開
        assert_eq!(
            (
                c.youtube_upload_token.as_str(),
                c.youtube_client.as_str(),
                c.youtube_privacy.as_str(),
                c.youtube_daily_limit
            ),
            (
                "~/.config/eq-replay/youtube-token.json",
                "~/.config/eq-replay/youtube-client.json",
                "private",
                3
            )
        );
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
            assert_eq!(
                c.events_source(),
                EventsSource::File(Path::new(&home).join("data/e.jsonl"))
            );
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
