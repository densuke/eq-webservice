//! 動画を 1 本ずつ上げる係。トークンの更新・1 日の本数・割り当ての超過・通信の失敗での間の空け方を持つ。
//! 通信は `Api` trait の後ろなので、テストは偽物でできる。ログに、トークン・秘密・ヘッダーは出さない。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::api::{Api, Insert, UploadError};
use super::limit::{self, Limits};
use super::token::{self, AuthError, Client, TokenFile};

/// 通信が失敗したときの、やり直すまでの間 (最初 1 分、倍々で最大 30 分)
const BACKOFF_FIRST_MS: u64 = 60_000;
const BACKOFF_MAX_MS: u64 = 30 * 60_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attempt {
    /// 上げた (動画の ID)
    Uploaded(String),
    /// 今は上げない (設定が無い・上限・割り当て・間を空けている・認証のやり直し待ち)。あとでまた試す
    Skipped,
    /// 断られた。同じ内容では通らない (理由)
    Rejected(String),
}

pub struct UploaderConfig {
    pub client_path: PathBuf,
    pub token_path: PathBuf,
    /// 1 日の本数を数える状態ファイル
    pub state_path: PathBuf,
    pub daily_limit: u32,
}

pub struct Uploader<A: Api> {
    api: A,
    cfg: UploaderConfig,
    /// この時刻まで通信しない
    wait_until_ms: u64,
    next_wait_ms: u64,
    /// `invalid_grant` になったときのトークンのファイルの更新時刻。ファイルが書き換わる (youtube-auth のやり直し) まで上げない
    dead_at: Option<Option<SystemTime>>,
    /// 同じ警告を続けて出さないための印
    warned_missing: bool,
    offline: bool,
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// 使えるアクセストークンを返す。期限内なら、ファイルに残したものを使う。無ければ更新して、ファイルに書く
pub async fn access_token<A: Api>(
    api: &A,
    client: &Client,
    token_path: &Path,
    now_ms: u64,
    force_refresh: bool,
) -> Result<String, AuthError> {
    let mut tok: TokenFile = token::load_token(token_path).map_err(|e| AuthError::Other(format!("{e:#}")))?;
    if !force_refresh {
        if let Some(t) = tok.usable_access(now_ms) {
            return Ok(t.to_string());
        }
    }
    let fresh = api.refresh(client, &tok.refresh_token, now_ms).await?;
    tok.access_token = Some(fresh.access_token.clone());
    tok.expires_at_ms = Some(fresh.expires_at_ms);
    if let Some(rt) = fresh.refresh_token {
        tok.refresh_token = rt;
    }
    // 書けなくても、このアップロードは続けられる (次はまた更新する)
    if let Err(e) = token::save_token(token_path, &tok) {
        tracing::warn!("youtube: トークンのファイルに書けません: {e:#}");
    }
    Ok(fresh.access_token)
}

impl<A: Api> Uploader<A> {
    pub fn new(api: A, cfg: UploaderConfig) -> Self {
        Uploader {
            api,
            cfg,
            wait_until_ms: 0,
            next_wait_ms: BACKOFF_FIRST_MS,
            dead_at: None,
            warned_missing: false,
            offline: false,
        }
    }

    /// 上げる準備ができているか (トークンのファイルがあるか)。無ければ、1 回だけ知らせる
    fn token_present(&mut self) -> bool {
        if self.cfg.token_path.exists() {
            self.warned_missing = false;
            return true;
        }
        if !self.warned_missing {
            self.warned_missing = true;
            tracing::warn!(
                "youtube: トークンのファイル ({}) がありません。YouTube に上げるには、先に eq-server youtube-auth --client <client.json> --token <token.json> を実行してください",
                self.cfg.token_path.display()
            );
        }
        false
    }

    /// 認証が取り消されて止まっているか。トークンのファイルが書き換わっていれば、止めるのをやめる
    fn auth_dead(&mut self) -> bool {
        match self.dead_at {
            Some(at) if at == mtime(&self.cfg.token_path) => true,
            Some(_) => {
                tracing::info!("youtube: トークンのファイルが書き換わったので、上げるのを再開します");
                self.dead_at = None;
                false
            }
            None => false,
        }
    }

    fn back_off(&mut self, now_ms: u64, why: &str) {
        self.wait_until_ms = now_ms + self.next_wait_ms;
        self.next_wait_ms = (self.next_wait_ms * 2).min(BACKOFF_MAX_MS);
        if !self.offline {
            self.offline = true;
            tracing::warn!("youtube: 上げられません。間を空けてやり直します: {why}");
        } else {
            tracing::debug!("youtube: 上げられません: {why}");
        }
    }

    fn mark_dead(&mut self) {
        self.dead_at = Some(mtime(&self.cfg.token_path));
        tracing::error!(
            "youtube: 認証が取り消されたか、期限が切れています (invalid_grant)。YouTube に上げるのを止めます。eq-server youtube-auth をやり直してください (トークンのファイルが書き換わると再開します)"
        );
    }

    /// 1 本上げてみる。条件がそろわなければ Skipped (何も通信しない)
    pub async fn try_upload(&mut self, meta: &Insert, video: &Path, now_ms: u64) -> Attempt {
        if now_ms < self.wait_until_ms || !self.token_present() || self.auth_dead() {
            return Attempt::Skipped;
        }
        let today = limit::pt_day(now_ms as i64);
        let limits = limit::load(&self.cfg.state_path).on(&today);
        if !limits.allows(self.cfg.daily_limit) {
            tracing::debug!(day = %today, count = limits.count, "youtube: 今日 (太平洋時間) はもう上げません");
            return Attempt::Skipped;
        }
        let client = match token::load_client(&self.cfg.client_path) {
            Ok(c) => c,
            Err(e) => {
                self.back_off(now_ms, &format!("{e:#}"));
                return Attempt::Skipped;
            }
        };
        let access = match access_token(&self.api, &client, &self.cfg.token_path, now_ms, false).await {
            Ok(t) => t,
            Err(e) => return self.on_auth_error(e, now_ms),
        };
        let mut result = self.api.upload(&access, meta, video).await;
        if result == Err(UploadError::Unauthorized) {
            // 期限の見込み違いかもしれない。更新して 1 回だけやり直す
            match access_token(&self.api, &client, &self.cfg.token_path, now_ms, true).await {
                Ok(t) => result = self.api.upload(&t, meta, video).await,
                Err(e) => return self.on_auth_error(e, now_ms),
            }
        }
        match result {
            Ok(id) => {
                self.finish_ok(limits);
                Attempt::Uploaded(id)
            }
            Err(UploadError::Quota) => {
                tracing::warn!(day = %today, "youtube: 割り当てを使い切りました。太平洋時間の 0 時まで上げません");
                self.save_limits(&limits.quota_hit());
                Attempt::Skipped
            }
            Err(UploadError::Unauthorized) => {
                self.back_off(now_ms, "認証が通りません (HTTP 401)");
                Attempt::Skipped
            }
            Err(UploadError::Transient(why)) => {
                self.back_off(now_ms, &why);
                Attempt::Skipped
            }
            Err(UploadError::Rejected(why)) => Attempt::Rejected(why),
        }
    }

    /// 上げた動画を再生リストに足す。本数・割り当て・間の空け方には関わらない。
    /// 失敗しても動画は上がっているので、上げ直さない (呼ぶ側は警告を出すだけ)
    pub async fn add_to_playlist(&self, playlist_id: &str, video_id: &str, now_ms: u64) -> Result<(), String> {
        let client = token::load_client(&self.cfg.client_path).map_err(|e| format!("{e:#}"))?;
        let access = access_token(&self.api, &client, &self.cfg.token_path, now_ms, false)
            .await
            .map_err(|e| format!("{e:?}"))?;
        let mut result = self.api.add_to_playlist(&access, playlist_id, video_id).await;
        if result == Err(UploadError::Unauthorized) {
            let fresh = access_token(&self.api, &client, &self.cfg.token_path, now_ms, true)
                .await
                .map_err(|e| format!("{e:?}"))?;
            result = self.api.add_to_playlist(&fresh, playlist_id, video_id).await;
        }
        result.map_err(|e| match e {
            UploadError::Rejected(why) if why.contains("insufficientPermissions") || why.contains("ACCESS_TOKEN_SCOPE_INSUFFICIENT") => format!(
                "{why}: トークンの許可の範囲が足りません。eq-server youtube-auth をやり直してください (再生リストへの追加には youtube.force-ssl が要ります)"
            ),
            other => format!("{other:?}"),
        })
    }

    fn on_auth_error(&mut self, e: AuthError, now_ms: u64) -> Attempt {
        match e {
            AuthError::InvalidGrant => self.mark_dead(),
            AuthError::Other(why) => self.back_off(now_ms, &why),
        }
        Attempt::Skipped
    }

    fn finish_ok(&mut self, limits: Limits) {
        self.save_limits(&limits.uploaded());
        self.wait_until_ms = 0;
        self.next_wait_ms = BACKOFF_FIRST_MS;
        if std::mem::take(&mut self.offline) {
            tracing::info!("youtube: 上げられるようになりました");
        }
    }

    fn save_limits(&self, l: &Limits) {
        if let Err(e) = limit::save(&self.cfg.state_path, l) {
            tracing::warn!("youtube: 本数の状態を書けません: {e:#}");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use super::super::token::Fresh;
    use super::*;

    const NOW: u64 = 1_790_000_000_000; // 2026-09 ごろ
    const HOUR: u64 = 3_600_000;

    /// 偽の Google: 応答を順に返し、呼ばれた内容を覚える
    #[derive(Default)]
    struct Fake {
        refresh_results: Mutex<VecDeque<Result<Fresh, AuthError>>>,
        upload_results: Mutex<VecDeque<Result<String, UploadError>>>,
        refresh_calls: Mutex<u32>,
        uploads: Mutex<Vec<(String, String)>>, // (アクセストークン, タイトル)
        playlist_results: Mutex<VecDeque<Result<(), UploadError>>>,
        playlist_adds: Mutex<Vec<(String, String, String)>>, // (アクセストークン, 再生リスト, 動画)
    }

    impl Api for &Fake {
        async fn refresh(&self, _c: &Client, _rt: &str, now_ms: u64) -> Result<Fresh, AuthError> {
            *self.refresh_calls.lock().unwrap() += 1;
            self.refresh_results.lock().unwrap().pop_front().unwrap_or(Ok(Fresh {
                access_token: "AT-new".into(),
                expires_at_ms: now_ms + HOUR,
                refresh_token: None,
            }))
        }
        async fn upload(&self, access: &str, meta: &Insert, _v: &Path) -> Result<String, UploadError> {
            self.uploads.lock().unwrap().push((access.into(), meta.title.clone()));
            self.upload_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok("VID".into()))
        }
        async fn add_to_playlist(&self, access: &str, playlist: &str, video: &str) -> Result<(), UploadError> {
            self.playlist_adds
                .lock()
                .unwrap()
                .push((access.into(), playlist.into(), video.into()));
            self.playlist_results.lock().unwrap().pop_front().unwrap_or(Ok(()))
        }
    }

    struct Env {
        dir: tempfile::TempDir,
    }

    impl Env {
        fn new() -> Env {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(
                dir.path().join("client.json"),
                r#"{"installed":{"client_id":"i","client_secret":"s"}}"#,
            )
            .unwrap();
            let env = Env { dir };
            env.write_token(r#"{"refresh_token":"RT"}"#);
            env
        }
        fn write_token(&self, json: &str) {
            std::fs::write(self.dir.path().join("token.json"), json).unwrap();
        }
        fn uploader<'a>(&self, fake: &'a Fake, daily_limit: u32) -> Uploader<&'a Fake> {
            Uploader::new(
                fake,
                UploaderConfig {
                    client_path: self.dir.path().join("client.json"),
                    token_path: self.dir.path().join("token.json"),
                    state_path: self.dir.path().join("state.json"),
                    daily_limit,
                },
            )
        }
    }

    fn meta(title: &str) -> Insert {
        Insert {
            title: title.into(),
            description: "d".into(),
            privacy: "private".into(),
            category: "25".into(),
        }
    }

    fn video() -> &'static Path {
        Path::new("/nonexistent.mp4") // 偽の Api は中身を読まない
    }

    #[tokio::test]
    async fn an_upload_refreshes_the_token_once_and_caches_it_in_the_file() {
        let env = Env::new();
        let fake = Fake::default();
        let mut u = env.uploader(&fake, 3);
        assert_eq!(
            u.try_upload(&meta("a"), video(), NOW).await,
            Attempt::Uploaded("VID".into())
        );
        assert_eq!(
            u.try_upload(&meta("b"), video(), NOW + 1000).await,
            Attempt::Uploaded("VID".into())
        );
        assert_eq!(
            *fake.refresh_calls.lock().unwrap(),
            1,
            "2 本目は、残したアクセストークンを使う"
        );
        let used: Vec<String> = fake.uploads.lock().unwrap().iter().map(|u| u.0.clone()).collect();
        assert_eq!(used, ["AT-new", "AT-new"]);
        let saved = token::load_token(&env.dir.path().join("token.json")).unwrap();
        assert_eq!(
            (saved.refresh_token.as_str(), saved.usable_access(NOW)),
            ("RT", Some("AT-new"))
        );
    }

    #[tokio::test]
    async fn invalid_grant_stops_uploading_without_looping_until_the_token_file_changes() {
        let env = Env::new();
        let fake = Fake::default();
        fake.refresh_results
            .lock()
            .unwrap()
            .push_back(Err(AuthError::InvalidGrant));
        let mut u = env.uploader(&fake, 3);
        assert_eq!(u.try_upload(&meta("a"), video(), NOW).await, Attempt::Skipped);
        for i in 1..5 {
            assert_eq!(
                u.try_upload(&meta("a"), video(), NOW + i * 5000).await,
                Attempt::Skipped
            );
        }
        assert_eq!(*fake.refresh_calls.lock().unwrap(), 1, "止めたあとは、通信しない");
        assert!(fake.uploads.lock().unwrap().is_empty());
        // youtube-auth をやり直した (トークンのファイルが書き換わった) ら、再開する
        std::thread::sleep(std::time::Duration::from_millis(20));
        env.write_token(r#"{"refresh_token":"RT2"}"#);
        assert_eq!(
            u.try_upload(&meta("a"), video(), NOW + HOUR).await,
            Attempt::Uploaded("VID".into())
        );
    }

    #[tokio::test]
    async fn a_missing_token_file_skips_without_any_request() {
        let env = Env::new();
        std::fs::remove_file(env.dir.path().join("token.json")).unwrap();
        let fake = Fake::default();
        let mut u = env.uploader(&fake, 3);
        assert_eq!(u.try_upload(&meta("a"), video(), NOW).await, Attempt::Skipped);
        assert_eq!(*fake.refresh_calls.lock().unwrap(), 0);
        assert!(fake.uploads.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_daily_limit_holds_and_the_next_pacific_day_resets_it() {
        let env = Env::new();
        let fake = Fake::default();
        let mut u = env.uploader(&fake, 2);
        // 太平洋時間の同じ日の中 (UTC 10:00 前後。PDT では 03:00 前後)
        let day = (NOW / 86_400_000) * 86_400_000 + 10 * HOUR;
        assert!(matches!(
            u.try_upload(&meta("1"), video(), day).await,
            Attempt::Uploaded(_)
        ));
        assert!(matches!(
            u.try_upload(&meta("2"), video(), day + HOUR).await,
            Attempt::Uploaded(_)
        ));
        assert_eq!(
            u.try_upload(&meta("3"), video(), day + 2 * HOUR).await,
            Attempt::Skipped
        );
        assert_eq!(fake.uploads.lock().unwrap().len(), 2);
        // 24 時間後は、太平洋時間でも次の日
        assert!(matches!(
            u.try_upload(&meta("3"), video(), day + 24 * HOUR).await,
            Attempt::Uploaded(_)
        ));
    }

    #[tokio::test]
    async fn quota_exceeded_stops_the_rest_of_the_pacific_day_even_below_the_limit() {
        let env = Env::new();
        let fake = Fake::default();
        fake.upload_results.lock().unwrap().push_back(Err(UploadError::Quota));
        let mut u = env.uploader(&fake, 10);
        let day = (NOW / 86_400_000) * 86_400_000 + 10 * HOUR;
        assert_eq!(u.try_upload(&meta("1"), video(), day).await, Attempt::Skipped);
        assert_eq!(u.try_upload(&meta("1"), video(), day + HOUR).await, Attempt::Skipped);
        assert_eq!(fake.uploads.lock().unwrap().len(), 1, "止めたあとは、通信しない");
        // 新しい Uploader (作る係の再起動) でも、状態ファイルで止まったまま
        let mut again = env.uploader(&fake, 10);
        assert_eq!(
            again.try_upload(&meta("1"), video(), day + 2 * HOUR).await,
            Attempt::Skipped
        );
        assert!(matches!(
            again.try_upload(&meta("1"), video(), day + 24 * HOUR).await,
            Attempt::Uploaded(_)
        ));
    }

    #[tokio::test]
    async fn transient_failures_back_off_with_doubling_waits_and_then_recover() {
        let env = Env::new();
        let fake = Fake::default();
        {
            let mut q = fake.upload_results.lock().unwrap();
            q.push_back(Err(UploadError::Transient("HTTP 503".into())));
            q.push_back(Err(UploadError::Transient("HTTP 503".into())));
        }
        let mut u = env.uploader(&fake, 3);
        assert_eq!(u.try_upload(&meta("a"), video(), NOW).await, Attempt::Skipped);
        // 1 分は通信しない
        assert_eq!(u.try_upload(&meta("a"), video(), NOW + 59_000).await, Attempt::Skipped);
        assert_eq!(fake.uploads.lock().unwrap().len(), 1);
        // 1 分後に 2 回目 (また失敗) -> 次は 2 分
        assert_eq!(u.try_upload(&meta("a"), video(), NOW + 60_000).await, Attempt::Skipped);
        assert_eq!(
            u.try_upload(&meta("a"), video(), NOW + 60_000 + 119_000).await,
            Attempt::Skipped
        );
        assert_eq!(fake.uploads.lock().unwrap().len(), 2);
        assert!(matches!(
            u.try_upload(&meta("a"), video(), NOW + 60_000 + 120_000).await,
            Attempt::Uploaded(_)
        ));
    }

    #[tokio::test]
    async fn a_401_refreshes_once_and_retries_with_the_new_token() {
        let env = Env::new();
        env.write_token(&format!(
            r#"{{"refresh_token":"RT","access_token":"OLD","expires_at_ms":{}}}"#,
            NOW + HOUR
        ));
        let fake = Fake::default();
        fake.upload_results
            .lock()
            .unwrap()
            .push_back(Err(UploadError::Unauthorized));
        let mut u = env.uploader(&fake, 3);
        assert!(matches!(
            u.try_upload(&meta("a"), video(), NOW).await,
            Attempt::Uploaded(_)
        ));
        let used: Vec<String> = fake.uploads.lock().unwrap().iter().map(|u| u.0.clone()).collect();
        assert_eq!(used, ["OLD", "AT-new"]);
    }

    #[tokio::test]
    async fn a_rejection_is_reported_and_does_not_count_as_an_upload() {
        let env = Env::new();
        let fake = Fake::default();
        fake.upload_results
            .lock()
            .unwrap()
            .push_back(Err(UploadError::Rejected("HTTP 400 invalidTitle".into())));
        let mut u = env.uploader(&fake, 1);
        assert_eq!(
            u.try_upload(&meta("a"), video(), NOW).await,
            Attempt::Rejected("HTTP 400 invalidTitle".into())
        );
        assert!(matches!(
            u.try_upload(&meta("b"), video(), NOW).await,
            Attempt::Uploaded(_)
        ));
    }

    #[tokio::test]
    async fn a_playlist_add_retries_once_on_401_and_does_not_count_toward_the_daily_limit() {
        let env = Env::new();
        let fake = Fake::default();
        fake.playlist_results
            .lock()
            .unwrap()
            .push_back(Err(UploadError::Unauthorized));
        let mut u = env.uploader(&fake, 1);
        assert_eq!(
            u.try_upload(&meta("a"), video(), NOW).await,
            Attempt::Uploaded("VID".into())
        );
        assert_eq!(u.add_to_playlist("PLx", "VID", NOW).await, Ok(()));
        let adds = fake.playlist_adds.lock().unwrap().clone();
        assert_eq!(adds.len(), 2, "401 のあと、更新して 1 回だけやり直す");
        assert_eq!(adds[1], ("AT-new".into(), "PLx".into(), "VID".into()));
        // 上限 1 本のまま: 再生リストへの追加は本数に数えない (2 本目は上限で止まる)
        assert_eq!(u.try_upload(&meta("b"), video(), NOW + 1000).await, Attempt::Skipped);
    }

    #[tokio::test]
    async fn a_playlist_add_without_the_scope_says_to_redo_the_consent() {
        let env = Env::new();
        let fake = Fake::default();
        fake.playlist_results
            .lock()
            .unwrap()
            .push_back(Err(UploadError::Rejected("HTTP 403 insufficientPermissions".into())));
        let u = env.uploader(&fake, 3);
        let err = u.add_to_playlist("PLx", "VID", NOW).await.unwrap_err();
        assert!(err.contains("youtube-auth をやり直して"), "{err}");
    }

    #[tokio::test]
    async fn a_refresh_that_returns_a_new_refresh_token_replaces_the_old_one() {
        let env = Env::new();
        let fake = Fake::default();
        fake.refresh_results.lock().unwrap().push_back(Ok(Fresh {
            access_token: "AT2".into(),
            expires_at_ms: NOW + HOUR,
            refresh_token: Some("RT-rotated".into()),
        }));
        let mut u = env.uploader(&fake, 3);
        u.try_upload(&meta("a"), video(), NOW).await;
        let saved = token::load_token(&env.dir.path().join("token.json")).unwrap();
        assert_eq!(saved.refresh_token, "RT-rotated");
    }
}
