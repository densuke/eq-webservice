//! できた動画を YouTube に上げる係との継ぎ目 (docs/replay-video.md 5.3)。
//! キューから 1 本選び、タイトル・説明文を作り、`youtube::Uploader` に渡し、結果をキューの JSON に残す。
//! 上げ方 (トークン・本数・割り当て・間の空け方) は youtube::uploader にある。

use std::path::PathBuf;

use anyhow::Context;

use super::config::{expand, WorkerConfig};
use super::queue::{self, Job};
use super::youtube_meta;
use crate::youtube::api::{Api, Google, Insert};
use crate::youtube::uploader::{Attempt, Uploader, UploaderConfig};

pub struct Publisher<A: Api> {
    uploader: Uploader<A>,
    queue_dir: PathBuf,
    done_dir: PathBuf,
    privacy: String,
    category: String,
    /// 上げた動画を足す再生リストの ID。空なら足さない
    playlist: String,
    delete_after_upload: bool,
}

impl Publisher<Google> {
    /// 設定でトークンのファイルが空なら、投稿しない (None)
    pub fn from_config(cfg: &WorkerConfig) -> anyhow::Result<Option<Self>> {
        if cfg.youtube_upload_token.is_empty() {
            tracing::info!("replay-worker: youtube_upload_token が空なので、YouTube には上げません");
            return Ok(None);
        }
        Ok(Some(Publisher::new(
            Google::new().context("YouTube の通信を用意できません")?,
            cfg,
        )))
    }
}

impl<A: Api> Publisher<A> {
    pub fn new(api: A, cfg: &WorkerConfig) -> Self {
        Publisher {
            uploader: Uploader::new(
                api,
                UploaderConfig {
                    client_path: expand(&cfg.youtube_client),
                    token_path: expand(&cfg.youtube_upload_token),
                    state_path: expand(&cfg.dir).join("youtube-state.json"),
                    daily_limit: cfg.youtube_daily_limit,
                },
            ),
            queue_dir: cfg.queue_dir(),
            done_dir: cfg.done_dir(),
            privacy: cfg.youtube_privacy.clone(),
            category: cfg.youtube_category.clone(),
            playlist: cfg.youtube_playlist.clone(),
            delete_after_upload: cfg.youtube_delete_after_upload,
        }
    }

    /// 上げるものがあれば、1 本だけ上げてみる (上げられない状況なら、何も通信しない)
    pub async fn run_once(&mut self, now_ms: u64) {
        let jobs = match queue::load_all(&self.queue_dir) {
            Ok(j) => j,
            Err(e) => return tracing::warn!("replay-worker: キューを読めません: {e:#}"),
        };
        let Some(job) = queue::next_upload(&jobs).cloned() else {
            return;
        };
        let file = self.done_dir.join(job.video.as_deref().unwrap_or_default());
        if !file.is_file() {
            let why = format!("動画のファイルがありません ({})", file.display());
            return self.save(queue::upload_rejected(job, &why, now_ms));
        }
        let meta = youtube_meta::build(&job.quakes, &job.chapters);
        let insert = Insert {
            title: meta.title,
            description: meta.description,
            privacy: self.privacy.clone(),
            category: self.category.clone(),
        };
        match self.uploader.try_upload(&insert, &file, now_ms).await {
            Attempt::Uploaded(id) => {
                tracing::info!(id = %job.id, video = %id, privacy = %self.privacy, "replay-worker: YouTube に上げました");
                self.save(queue::uploaded(job.clone(), id.clone(), now_ms));
                if !self.playlist.is_empty() {
                    // 動画はもう上がっている。足せなくても上げ直さない (Studio で手で足せる)
                    match self.uploader.add_to_playlist(&self.playlist, &id, now_ms).await {
                        Ok(()) => {
                            tracing::info!(id = %job.id, video = %id, playlist = %self.playlist, "replay-worker: 再生リストに足しました")
                        }
                        Err(why) => {
                            tracing::warn!(id = %job.id, video = %id, playlist = %self.playlist, "replay-worker: 再生リストに足せません: {why}")
                        }
                    }
                }
                if self.delete_after_upload {
                    if let Err(e) = std::fs::remove_file(&file) {
                        tracing::warn!("replay-worker: 上げた動画を消せません: {e}");
                    }
                }
            }
            Attempt::Skipped => {}
            Attempt::Rejected(why) => {
                tracing::warn!(id = %job.id, "replay-worker: YouTube に断られました: {why}");
                self.save(queue::upload_rejected(job, &why, now_ms));
            }
        }
    }

    fn save(&self, job: Job) {
        if let Err(e) = queue::save(&self.queue_dir, &job) {
            tracing::warn!("replay-worker: キューに書けません: {e:#}");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::path::Path;
    use std::sync::Mutex;

    use super::super::super::testkit::*;
    use super::super::detect::{groups, Quake, Rules};
    use super::super::queue::{finish, Outcome, Policy, State};
    use super::*;
    use crate::youtube::api::UploadError;
    use crate::youtube::token::{AuthError, Client, Fresh};

    const NOW: u64 = 1_790_000_000_000;

    #[derive(Default)]
    struct Fake {
        results: Mutex<VecDeque<Result<String, UploadError>>>,
        titles: Mutex<Vec<String>>,
        playlist_results: Mutex<VecDeque<Result<(), UploadError>>>,
        playlist_adds: Mutex<Vec<(String, String)>>, // (再生リスト, 動画)
    }

    impl Api for &Fake {
        async fn refresh(&self, _: &Client, _: &str, _: u64) -> Result<Fresh, AuthError> {
            Err(AuthError::Other("このテストでは更新しない".into()))
        }
        async fn upload(&self, _: &str, meta: &Insert, _: &Path) -> Result<String, UploadError> {
            self.titles.lock().unwrap().push(meta.title.clone());
            self.results.lock().unwrap().pop_front().unwrap_or(Ok("VID".into()))
        }
        async fn add_to_playlist(&self, _: &str, playlist: &str, video: &str) -> Result<(), UploadError> {
            self.playlist_adds.lock().unwrap().push((playlist.into(), video.into()));
            self.playlist_results.lock().unwrap().pop_front().unwrap_or(Ok(()))
        }
    }

    struct Env {
        dir: tempfile::TempDir,
        cfg: WorkerConfig,
    }

    fn env(delete_after: bool) -> Env {
        env_with(delete_after, "")
    }

    fn env_with(delete_after: bool, playlist: &str) -> Env {
        let dir = tempfile::tempdir().unwrap();
        let p = |n: &str| dir.path().join(n);
        std::fs::write(
            p("client.json"),
            r#"{"installed":{"client_id":"i","client_secret":"s"}}"#,
        )
        .unwrap();
        // 使えるアクセストークンが残っているので、更新の通信は要らない
        std::fs::write(
            p("token.json"),
            format!(
                r#"{{"refresh_token":"RT","access_token":"AT","expires_at_ms":{}}}"#,
                NOW + 3_600_000
            ),
        )
        .unwrap();
        let cfg = WorkerConfig {
            dir: dir.path().display().to_string(),
            youtube_upload_token: p("token.json").display().to_string(),
            youtube_client: p("client.json").display().to_string(),
            youtube_delete_after_upload: delete_after,
            youtube_playlist: playlist.into(),
            ..WorkerConfig::default()
        };
        std::fs::create_dir_all(cfg.done_dir()).unwrap();
        Env { dir, cfg }
    }

    /// できた動画 (mp4 も置く) を 1 本キューに積む
    fn enqueue_done(env: &Env, origin_ms: i64) -> Job {
        let q = Quake {
            origin_ms,
            lat: None,
            lon: None,
            max_scale: 30,
            warning: false,
            name: "熊本県熊本地方".into(),
            last_recv_ms: origin_ms as u64 + 60_000,
            magnitude: Some(4.5),
            depth_km: Some(10),
        };
        let g = groups(&[q], &Rules::from(&env.cfg)).remove(0);
        let policy = Policy {
            max_retries: 5,
            retry_wait_ms: 0,
        };
        let job = finish(
            Job::new(&g, 1),
            Outcome::Made {
                video: format!("{}.mp4", g.id()),
                chapters: vec![],
            },
            2,
            &policy,
        );
        std::fs::write(env.cfg.done_dir().join(job.video.as_ref().unwrap()), b"mp4").unwrap();
        queue::save(&env.cfg.queue_dir(), &job).unwrap();
        job
    }

    fn reload(env: &Env, id: &str) -> Job {
        queue::load_all(&env.cfg.queue_dir())
            .unwrap()
            .into_iter()
            .find(|j| j.id == id)
            .unwrap()
    }

    #[tokio::test]
    async fn a_made_video_is_uploaded_once_and_recorded_in_the_queue() {
        let env = env(false);
        let job = enqueue_done(&env, T0);
        let fake = Fake::default();
        let mut p = Publisher::new(&fake, &env.cfg);
        p.run_once(NOW).await;
        let done = reload(&env, &job.id);
        assert_eq!(
            (done.state, done.youtube_id.as_deref(), done.uploaded_ms),
            (State::Uploaded, Some("VID"), Some(NOW))
        );
        assert!(fake.titles.lock().unwrap()[0].starts_with("【地震の記録】"));
        // もう一度回しても、二重には上げない。動画は消さない (既定)
        p.run_once(NOW + 1000).await;
        assert_eq!(fake.titles.lock().unwrap().len(), 1);
        assert!(env.cfg.done_dir().join(job.video.unwrap()).exists());
    }

    #[tokio::test]
    async fn an_uploaded_video_goes_into_the_playlist_and_a_failed_add_is_not_retried() {
        let env = env_with(false, "PLx");
        let first = enqueue_done(&env, T0);
        let fake = Fake::default();
        let mut p = Publisher::new(&fake, &env.cfg);
        p.run_once(NOW).await;
        assert_eq!(
            fake.playlist_adds.lock().unwrap().clone(),
            [("PLx".to_string(), "VID".to_string())]
        );
        assert_eq!(reload(&env, &first.id).state, State::Uploaded);
        // 足すのに失敗しても、上げたことは残り、上げ直さない
        let second = enqueue_done(&env, T0 + 86_400_000);
        fake.playlist_results
            .lock()
            .unwrap()
            .push_back(Err(UploadError::Rejected("HTTP 403".into())));
        p.run_once(NOW + 1000).await;
        p.run_once(NOW + 2000).await;
        assert_eq!(reload(&env, &second.id).state, State::Uploaded);
        assert_eq!(fake.titles.lock().unwrap().len(), 2);
        assert_eq!(fake.playlist_adds.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn no_playlist_configured_means_no_playlist_call() {
        let env = env(false);
        enqueue_done(&env, T0);
        let fake = Fake::default();
        Publisher::new(&fake, &env.cfg).run_once(NOW).await;
        assert!(fake.playlist_adds.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_video_can_be_deleted_after_upload_when_configured() {
        let env = env(true);
        let job = enqueue_done(&env, T0);
        let fake = Fake::default();
        Publisher::new(&fake, &env.cfg).run_once(NOW).await;
        assert!(!env.cfg.done_dir().join(job.video.unwrap()).exists());
    }

    #[tokio::test]
    async fn a_rejected_upload_is_counted_and_left_in_done() {
        let env = env(false);
        let job = enqueue_done(&env, T0);
        let fake = Fake::default();
        fake.results
            .lock()
            .unwrap()
            .push_back(Err(UploadError::Rejected("HTTP 400".into())));
        Publisher::new(&fake, &env.cfg).run_once(NOW).await;
        let after = reload(&env, &job.id);
        assert_eq!(
            (after.state, after.upload_failures, after.youtube_id),
            (State::Done, 1, None)
        );
    }

    #[tokio::test]
    async fn a_missing_video_file_is_counted_instead_of_looping() {
        let env = env(false);
        let job = enqueue_done(&env, T0);
        std::fs::remove_file(env.cfg.done_dir().join(job.video.as_ref().unwrap())).unwrap();
        let fake = Fake::default();
        Publisher::new(&fake, &env.cfg).run_once(NOW).await;
        assert!(fake.titles.lock().unwrap().is_empty());
        assert_eq!(reload(&env, &job.id).upload_failures, 1);
    }

    #[tokio::test]
    async fn nothing_is_uploaded_when_the_token_file_is_missing() {
        let env = env(false);
        let job = enqueue_done(&env, T0);
        std::fs::remove_file(env.dir.path().join("token.json")).unwrap();
        let fake = Fake::default();
        Publisher::new(&fake, &env.cfg).run_once(NOW).await;
        assert!(fake.titles.lock().unwrap().is_empty());
        assert_eq!(reload(&env, &job.id).state, State::Done);
    }

    #[test]
    fn no_token_configured_means_no_publisher() {
        assert!(Publisher::from_config(&WorkerConfig::default()).unwrap().is_none());
    }
}
