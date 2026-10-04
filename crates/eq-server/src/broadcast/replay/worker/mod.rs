//! 再現動画を自動で作る係 (`eq-server replay-worker`。docs/replay-video.md の 5 章、R3.3a)。
//! systemd のユーザー単位で常駐し、記録から動画にする地震を見つけて (detect)、キューに積み (queue)、
//! 条件がそろったときだけ 1 つずつ `replay-video` を作らせる (job)。作っている間は配信の状態・e2 の詰まり・
//! YouTube の健全性を見張り (decide)、地震の画面になれば止め、重くなれば (既定は) 止める。
//! 配信と本番が最優先で、動画作りはいつでも捨ててよい仕事。判断は decide.rs・detect.rs・queue.rs の純粋な関数で、ここは外とのやり取りだけ。

mod config;
mod decide;
mod detect;
mod feed;
mod health;
mod hours;
mod job;
mod queue;
mod scan;

use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::Context;
use tokio::process::{Child, Command};

use super::source::{self, ARCHIVE_PAUSE, FILE_MAX_EVENTS};
use crate::archive;
use crate::broadcast::calm_state;
use crate::quake::Event;
use config::{EventsSource, Gate, Runner, WorkerConfig};
use decide::{Action, Seen, StartRules, Why};
use health::{Checker, Health};
use queue::{Job, Outcome, Policy, State};

const USAGE: &str = "\
usage: eq-server replay-worker [replay.toml]                         (常駐して、動画を自動で作る)
       eq-server replay-worker --scan <events.jsonl | URL> [--hours 24] [replay.toml]
                                                                     (検知とまとめだけを試して出す。何も書かない)";

/// 止めた単位が終わるのを待つ時間 (これを過ぎたら、stop で押し切る)
const KILL_WAIT: Duration = Duration::from_secs(30);

pub async fn run(args: &[String]) -> anyhow::Result<()> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    let (mut scan, mut hours, mut config) = (None, 24u64, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--scan" => scan = Some(it.next().context("--scan needs a value")?.clone()),
            "--hours" => hours = it.next().context("--hours needs a value")?.parse().context("--hours")?,
            other if !other.starts_with('-') && config.is_none() => config = Some(other.to_string()),
            other => anyhow::bail!("unknown argument {other:?}\n\n{USAGE}"),
        }
    }
    let cfg = config::load(config.as_deref())?;
    match scan {
        Some(spec) => scan::run(&spec, hours, &cfg).await,
        None => Worker::new(cfg)?.run().await,
    }
}

/// 作っている 1 本
struct Running {
    job: Job,
    child: Child,
    unit: String,
    out: PathBuf,
    chapters: PathBuf,
    frozen_since: Option<Instant>,
    /// 止めたとき、その理由 (止めたあとの結果)
    stopped: Option<Outcome>,
}

struct Worker {
    cfg: WorkerConfig,
    rules: detect::Rules,
    policy: Policy,
    state_path: PathBuf,
    queue_dir: PathBuf,
    health: Option<Checker>,
    last_scan: Option<Instant>,
    /// 記録を取れない状態が続いているか (警告は、切れたときの 1 回だけ)
    offline: bool,
    running: Option<Running>,
}

fn now_ms() -> u64 {
    calm_state::now_ms()
}

impl Worker {
    fn new(cfg: WorkerConfig) -> anyhow::Result<Self> {
        let health = if cfg.gate == Gate::None {
            tracing::info!(
                "replay-worker: gate = \"none\" なので、配信の状態・PSI・メモリ・YouTube の健全性は見ません"
            );
            None
        } else if cfg.youtube_token.is_empty() {
            tracing::info!("replay-worker: youtube_token が空なので、YouTube の健全性は見ません");
            None
        } else {
            Some(Checker::new(
                config::expand(&cfg.youtube_token),
                cfg.stream_name.clone(),
                Duration::from_secs(cfg.health_secs),
            )?)
        };
        Ok(Worker {
            rules: detect::Rules::from(&cfg),
            policy: Policy {
                max_retries: cfg.max_retries,
                retry_wait_ms: cfg.retry_wait_min * 60_000,
            },
            state_path: calm_state::path_of(&cfg.state_file),
            queue_dir: cfg.queue_dir(),
            health,
            last_scan: None,
            offline: false,
            running: None,
            cfg,
        })
    }

    async fn run(mut self) -> anyhow::Result<()> {
        self.prepare().await?;
        tracing::info!(
            queue = %self.queue_dir.display(),
            state = %self.state_path.display(),
            "replay-worker: 始めます"
        );
        let stop = crate::shutdown_signal();
        tokio::pin!(stop);
        let mut tick = tokio::time::interval(Duration::from_secs(self.cfg.tick_secs));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = tick.tick() => self.tick().await,
                _ = &mut stop => break,
            }
        }
        // 作っている途中なら、止めて待ちに戻す (作りかけを残さない)
        if let Some(mut r) = self.running.take() {
            self.stop(&mut r, Outcome::Interrupted).await;
            self.conclude(r);
        }
        tracing::info!("replay-worker: 止めます");
        Ok(())
    }

    /// 置き場を作り、前の作る係が落ちて残したものを片づける
    async fn prepare(&self) -> anyhow::Result<()> {
        for d in [self.queue_dir.clone(), self.cfg.work_dir(), self.cfg.done_dir()] {
            std::fs::create_dir_all(&d).with_context(|| format!("creating {}", d.display()))?;
        }
        // 残っている作りかけの単位と、作りかけのファイル
        // (該当する単位が無いと失敗するので、結果は見ない)
        if self.cfg.runner == Runner::Systemd {
            let _ = Command::new(&self.cfg.systemctl)
                .args(["--user", "stop", "eq-replay-job-*"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }
        if let Ok(entries) = std::fs::read_dir(self.cfg.work_dir()) {
            entries.flatten().for_each(|e| {
                let _ = std::fs::remove_file(e.path());
            });
        }
        for job in queue::load_all(&self.queue_dir)? {
            if job.state == State::Making {
                queue::save(&self.queue_dir, &queue::recover(job, now_ms()))?;
            }
        }
        Ok(())
    }

    async fn tick(&mut self) {
        if self.running.is_some() {
            self.watch().await;
        } else {
            self.idle().await;
        }
    }

    /// 配信の状態・e2 の詰まり・健全性から、今の様子を作る
    fn seen(&self, health: Health) -> Seen {
        // gate = "none" は、何も読まない (Linux の /proc も要らない)。判断 (decide) もこの値を見ない
        if self.cfg.gate == Gate::None {
            return Seen {
                screen: calm_state::Screen::Unknown,
                congested: false,
                health: Health::Unknown,
                mem_available_mb: None,
            };
        }
        Seen {
            screen: calm_state::screen(calm_state::read(&self.state_path).as_ref(), now_ms()),
            congested: crate::broadcast::psi::congested_now(self.cfg.psi_limit),
            health,
            mem_available_mb: crate::broadcast::psi::mem_available_now(),
        }
    }

    async fn health(&mut self) -> Health {
        match &mut self.health {
            Some(c) => c.check().await,
            None => Health::Unknown,
        }
    }

    /// 作っていないとき: 記録を見直し、条件がそろっていれば次を作り始める
    async fn idle(&mut self) {
        if self
            .last_scan
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(self.cfg.scan_secs))
        {
            self.last_scan = Some(Instant::now());
            self.scan().await;
        }
        let jobs = match queue::load_all(&self.queue_dir) {
            Ok(j) => j,
            Err(e) => return tracing::warn!("replay-worker: キューを読めません: {e:#}"),
        };
        let now = now_ms();
        let Some(job) = queue::next_job(&jobs, now).cloned() else {
            return;
        };
        let rules = StartRules {
            gate: self.cfg.gate,
            calm_ms: self.cfg.calm_min * 60_000,
            hours: self.cfg.start_hours(),
            min_mem_mb: self.cfg.min_mem_mb,
        };
        // 健全性は API を使うので、ほかの条件がそろってから見る
        if !decide::may_start(&self.seen(Health::Unknown), now, &rules) {
            return;
        }
        let health = self.health().await;
        if decide::may_start(&self.seen(health), now_ms(), &rules) {
            self.start(job).await;
        }
    }

    /// 記録から、閉じたまとまりを見つけて、キューに積む。
    /// ファイルなら最近 lookback_hours を読む。URL なら、前に見終えた時刻から今までを取る (feed.rs)
    async fn scan(&mut self) {
        let now = now_ms();
        let res = match self.cfg.events_source() {
            EventsSource::File(path) => self.scan_file(&path, now).await,
            EventsSource::Url(base) => {
                let window = feed::Window::from(&self.cfg);
                feed::catch_up(
                    &self.cfg.checkpoint_path(),
                    now,
                    &window,
                    ARCHIVE_PAUSE,
                    |a, b| source::from_archive(&base, a, b),
                    |events| self.enqueue(&events, now),
                )
                .await
            }
        };
        self.note_scan(res);
    }

    async fn scan_file(&self, path: &std::path::Path, now: u64) -> anyhow::Result<()> {
        let from = now.saturating_sub(self.cfg.lookback_hours * 3_600_000);
        let events = archive::read_range_upto(path, from, now, FILE_MAX_EVENTS).await?;
        self.enqueue(&events, now);
        Ok(())
    }

    /// 見直しの結果を記録する。取れない状態 (オフライン・スリープ明け) は、切れたときに警告を 1 回だけ出し、
    /// 続く間は静かに次の見直しを待つ。戻ったら 1 回知らせる
    fn note_scan(&mut self, res: anyhow::Result<()>) {
        match res {
            Ok(()) if self.offline => {
                self.offline = false;
                tracing::info!("replay-worker: 記録を取れるようになりました");
            }
            Ok(()) => {}
            Err(e) if !self.offline => {
                self.offline = true;
                tracing::warn!("replay-worker: 記録を取れません。次の見直しでやり直します: {e:#}");
            }
            Err(e) => tracing::debug!("replay-worker: 記録を取れません: {e:#}"),
        }
    }

    /// 閉じたまとまりのうち、キューに無いものを積む。全部うまくいったか (false なら、次の見直しでやり直す)
    fn enqueue(&self, events: &[Event], now: u64) -> bool {
        let existing = match queue::load_all(&self.queue_dir) {
            Ok(j) => j,
            Err(e) => {
                tracing::warn!("replay-worker: キューを読めません: {e:#}");
                return false;
            }
        };
        let mut all_saved = true;
        for job in queue::new_jobs(events, &self.rules, now, &existing) {
            match queue::save(&self.queue_dir, &job) {
                Ok(()) => tracing::info!(id = %job.id, quakes = job.quakes.len(), "replay-worker: キューに積みました"),
                Err(e) => {
                    all_saved = false;
                    tracing::warn!("replay-worker: キューに積めません: {e:#}");
                }
            }
        }
        all_saved
    }

    async fn start(&mut self, mut job: Job) {
        let now = now_ms();
        let (out, chapters) = job::work_files(&self.cfg, &job.id);
        let _ = std::fs::remove_file(&out);
        let unit = job::unit_name(&job.id, now / 1000);
        let spawned = std::env::current_exe()
            .map_err(anyhow::Error::from)
            .and_then(|exe| self.spawn(&job, &unit, &exe, &out, &chapters));
        match spawned {
            Ok(child) => {
                tracing::info!(id = %job.id, unit = %unit, "replay-worker: 作り始めます");
                job.state = State::Making;
                job.updated_ms = now;
                if let Err(e) = queue::save(&self.queue_dir, &job) {
                    tracing::warn!("replay-worker: キューに書けません: {e:#}");
                }
                self.running = Some(Running {
                    job,
                    child,
                    unit,
                    out,
                    chapters,
                    frozen_since: None,
                    stopped: None,
                });
            }
            Err(e) => {
                tracing::warn!("replay-worker: 起動できません: {e:#}");
                let failed = queue::finish(job, Outcome::Failed(format!("{e:#}")), now, &self.policy);
                let _ = queue::save(&self.queue_dir, &failed);
            }
        }
    }

    /// 子を起動する。systemd は一時的なユーザー単位 (縛りと凍結のため)、inline は直接の子プロセス
    fn spawn(
        &self,
        job: &Job,
        unit: &str,
        exe: &std::path::Path,
        out: &std::path::Path,
        chapters: &std::path::Path,
    ) -> anyhow::Result<Child> {
        let (program, args) = match self.cfg.runner {
            Runner::Systemd => {
                let cwd = std::env::current_dir()?;
                (
                    self.cfg.systemd_run.clone(),
                    job::systemd_run_args(&self.cfg, job, unit, exe, &cwd, out, chapters),
                )
            }
            Runner::Inline => {
                let mut cmd = job::inline_command(&self.cfg, job, exe, out, chapters);
                (cmd.remove(0), cmd)
            }
        };
        Command::new(&program)
            .args(args)
            .stdin(Stdio::null())
            // 作る係が落ちたときに、子を残さない (inline では、これが唯一の後始末)
            .kill_on_drop(self.cfg.runner == Runner::Inline)
            .spawn()
            .with_context(|| format!("starting {program}"))
    }

    /// 作っている間の見張り
    async fn watch(&mut self) {
        let Some(mut r) = self.running.take() else { return };
        match r.child.try_wait() {
            Ok(Some(status)) => {
                let ended = status.success();
                self.finish_run(r, ended);
                return;
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!("replay-worker: 子の様子を読めません: {e}");
                self.stop(&mut r, Outcome::Failed(format!("{e}"))).await;
                self.conclude(r);
                return;
            }
        }
        let health = self.health().await;
        let seen = self.seen(health);
        let give_up = Duration::from_secs(self.cfg.freeze_give_up_min * 60);
        match decide::watch(
            self.cfg.gate,
            &seen,
            r.frozen_since.map(|t| t.elapsed()),
            give_up,
            self.cfg.on_busy,
        ) {
            Action::Keep => {}
            Action::Freeze => {
                tracing::info!(id = %r.job.id, ?seen, "replay-worker: 凍結します");
                if self.systemctl(&["freeze", &r.unit]).await {
                    r.frozen_since = Some(Instant::now());
                } else {
                    self.stop(&mut r, Outcome::Failed("凍結できませんでした".into())).await;
                    self.conclude(r);
                    return;
                }
            }
            Action::Thaw => {
                tracing::info!(id = %r.job.id, "replay-worker: 解凍します");
                if self.systemctl(&["thaw", &r.unit]).await {
                    r.frozen_since = None;
                } else {
                    self.stop(&mut r, Outcome::Failed("解凍できませんでした".into())).await;
                    self.conclude(r);
                    return;
                }
            }
            Action::Kill(why) => {
                tracing::info!(id = %r.job.id, ?why, ?seen, "replay-worker: 止めて待ちに戻します");
                let outcome = match why {
                    Why::Quake => Outcome::Interrupted,
                    // 重くて止めたものは、やり直しに数えて、retry_wait_min の間を空ける
                    Why::Busy => Outcome::Failed("e2 が重い (詰まり・YouTube の健全性) ので止めました".into()),
                    Why::FrozenTooLong => Outcome::FrozenTooLong,
                };
                self.stop(&mut r, outcome).await;
                self.conclude(r);
                return;
            }
        }
        self.running = Some(r);
    }

    /// 子を止める。systemd は、単位を止める (凍結していても確実に止まるよう SIGKILL。終わらなければ stop で押し切る)。
    /// inline は、子を SIGKILL する
    async fn stop(&self, r: &mut Running, outcome: Outcome) {
        r.stopped = Some(outcome);
        if self.cfg.runner == Runner::Inline {
            let _ = r.child.kill().await;
            return;
        }
        self.systemctl(&["kill", "--signal=SIGKILL", &r.unit]).await;
        if tokio::time::timeout(KILL_WAIT, r.child.wait()).await.is_err() {
            tracing::warn!(unit = %r.unit, "replay-worker: 止まらないので stop します");
            self.systemctl(&["stop", &r.unit]).await;
            let _ = r.child.kill().await;
        }
    }

    fn finish_run(&self, r: Running, exited_ok: bool) {
        let mut r = r;
        if !exited_ok && r.stopped.is_none() {
            r.stopped = Some(Outcome::Failed("replay-video が失敗しました".into()));
        }
        self.conclude(r);
    }

    /// 終わった 1 本の後始末: できた動画を done/ に移し、キューを更新し、作りかけを消す
    fn conclude(&self, r: Running) {
        let now = now_ms();
        let Running {
            job,
            out,
            chapters,
            stopped,
            ..
        } = r;
        let outcome = stopped.unwrap_or_else(|| self.collect(&job, &out, &chapters));
        let made = matches!(outcome, Outcome::Made { .. });
        tracing::info!(id = %job.id, ?outcome, "replay-worker: 終わりました");
        if !made {
            let _ = std::fs::remove_file(&out);
        }
        let mut audio = out.as_os_str().to_owned();
        audio.push(".audio.m4a");
        let _ = std::fs::remove_file(audio);
        let _ = std::fs::remove_file(&chapters);
        let next = queue::finish(job, outcome, now, &self.policy);
        if let Err(e) = queue::save(&self.queue_dir, &next) {
            tracing::warn!("replay-worker: キューに書けません: {e:#}");
        }
    }

    /// 正常に終わった動画を done/ に置く (置き換えで)。チャプターを読む
    fn collect(&self, job: &Job, out: &PathBuf, chapters: &PathBuf) -> Outcome {
        let name = format!("{}.mp4", job.id);
        let dest = self.cfg.done_dir().join(&name);
        if let Err(e) = std::fs::rename(out, &dest) {
            return Outcome::Failed(format!("できた動画を {} に移せません: {e}", dest.display()));
        }
        let chapters = std::fs::read(chapters)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Outcome::Made { video: name, chapters }
    }

    /// `systemctl --user <args>`。成功したか
    async fn systemctl(&self, args: &[&str]) -> bool {
        let res = Command::new(&self.cfg.systemctl)
            .arg("--user")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .status()
            .await;
        match res {
            Ok(s) if s.success() => true,
            Ok(s) => {
                tracing::warn!(?args, "replay-worker: systemctl が失敗しました ({s})");
                false
            }
            Err(e) => {
                tracing::warn!(?args, "replay-worker: systemctl を起動できません: {e}");
                false
            }
        }
    }
}
