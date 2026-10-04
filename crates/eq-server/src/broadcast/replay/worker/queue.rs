//! 動画のキュー: 1 つのまとまりごとの JSON を queue/ に置く (docs/replay-video.md 5.2 の 2)。
//! 人が見てもわかる形 (整形した JSON) にする。状態の移り変わりは純粋な関数で、ファイルの読み書きは最後に分ける。

use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use super::super::plan::Chapter;
use super::detect::{self, Group, Quake, Rules};
use crate::quake::Event;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// 作るのを待っている
    Waiting,
    /// 作っている
    Making,
    /// できた (done/ に mp4 がある)
    Done,
    /// 上げた (R3.3b)
    Uploaded,
    /// やり直しを使い切った
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    /// まとまりの名前 (最初の地震の発生時刻。ファイル名にも使う)
    pub id: String,
    pub state: State,
    /// 動画にする報の範囲 (received_at_ms)
    pub from_ms: u64,
    pub to_ms: u64,
    /// まとまりの地震 (発生の早い順)
    pub quakes: Vec<Quake>,
    pub created_ms: u64,
    pub updated_ms: u64,
    /// 失敗でやり直した回数 (地震の画面で止めたものは数えない)
    pub retries: u32,
    /// この時刻まで作り始めない (失敗のあとの間)
    pub not_before_ms: u64,
    /// 直近の失敗の理由
    #[serde(default)]
    pub error: Option<String>,
    /// できた動画のファイル名 (done/ の下)
    #[serde(default)]
    pub video: Option<String>,
    /// 各地震の始まりの、動画の中の時刻 (説明文のチャプターに使う)
    #[serde(default)]
    pub chapters: Vec<Chapter>,
    /// 上げた YouTube の動画の ID (あれば二重に上げない)。古いファイルには無い
    #[serde(default)]
    pub youtube_id: Option<String>,
    /// 上げた時刻 (epoch ミリ秒)
    #[serde(default)]
    pub uploaded_ms: Option<u64>,
    /// 上げるのを断られた回数 (4xx。通信の失敗は数えない)
    #[serde(default)]
    pub upload_failures: u32,
}

impl Job {
    pub fn new(group: &Group, now_ms: u64) -> Job {
        let (from_ms, to_ms) = group.range_ms();
        Job {
            id: group.id(),
            state: State::Waiting,
            from_ms,
            to_ms,
            quakes: group.quakes.clone(),
            created_ms: now_ms,
            updated_ms: now_ms,
            retries: 0,
            not_before_ms: 0,
            error: None,
            video: None,
            chapters: Vec::new(),
            youtube_id: None,
            uploaded_ms: None,
            upload_failures: 0,
        }
    }
}

/// 作る係が終わった理由
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Made {
        video: String,
        chapters: Vec<Chapter>,
    },
    Failed(String),
    /// 凍結が長く続いて止めた
    FrozenTooLong,
    /// 地震の画面になった・作る係が止まるなどで止めた (作り直すだけ。失敗には数えない)
    Interrupted,
}

#[derive(Debug, Clone, Copy)]
pub struct Policy {
    pub max_retries: u32,
    pub retry_wait_ms: u64,
}

/// 作り終えた (または止めた) 動画の状態を決める
pub fn finish(mut job: Job, outcome: Outcome, now_ms: u64, p: &Policy) -> Job {
    job.updated_ms = now_ms;
    let failure = match outcome {
        Outcome::Made { video, chapters } => {
            job.state = State::Done;
            job.video = Some(video);
            job.chapters = chapters;
            job.error = None;
            return job;
        }
        Outcome::Interrupted => {
            job.state = State::Waiting;
            return job;
        }
        Outcome::Failed(why) => why,
        Outcome::FrozenTooLong => "凍結が長く続いたので止めました".to_string(),
    };
    job.retries += 1;
    job.error = Some(failure);
    if job.retries > p.max_retries {
        job.state = State::Failed;
    } else {
        job.state = State::Waiting;
        job.not_before_ms = now_ms + p.retry_wait_ms;
    }
    job
}

/// 作る係を始めるとき、作り中のまま残っていたものを待ちに戻す (前の作る係が落ちた)
pub fn recover(mut job: Job, now_ms: u64) -> Job {
    if job.state == State::Making {
        job.state = State::Waiting;
        job.updated_ms = now_ms;
    }
    job
}

/// 次に作るもの: 待ちで、待つ時刻を過ぎている中で、いちばん古いもの
pub fn next_job(jobs: &[Job], now_ms: u64) -> Option<&Job> {
    jobs.iter()
        .filter(|j| j.state == State::Waiting && j.not_before_ms <= now_ms)
        .min_by_key(|j| (j.from_ms, j.created_ms))
}

/// 上げる回数の上限 (断られた回数がこれに達したら、諦める)
pub const MAX_UPLOAD_FAILURES: u32 = 5;

/// 次に上げるもの: できていて、まだ上げていなくて、断られ続けていない中で、いちばん古いもの
pub fn next_upload(jobs: &[Job]) -> Option<&Job> {
    jobs.iter()
        .filter(|j| {
            j.state == State::Done
                && j.video.is_some()
                && j.youtube_id.is_none()
                && j.upload_failures < MAX_UPLOAD_FAILURES
        })
        .min_by_key(|j| (j.from_ms, j.created_ms))
}

/// 上げた (動画の ID と時刻を残す。これで二重に上げない)
pub fn uploaded(mut job: Job, youtube_id: String, now_ms: u64) -> Job {
    job.state = State::Uploaded;
    job.youtube_id = Some(youtube_id);
    job.uploaded_ms = Some(now_ms);
    job.error = None;
    job.updated_ms = now_ms;
    job
}

/// 上げるのを断られた (同じ内容では通らないので、回数を数えて、上限で諦める)
pub fn upload_rejected(mut job: Job, why: &str, now_ms: u64) -> Job {
    job.upload_failures += 1;
    job.error = Some(format!("YouTube に上げられません: {why}"));
    job.updated_ms = now_ms;
    job
}

/// まとまりの地震のどれかが、もうキューにあるか (同じ地震を二重に積まない。記録を見る範囲の端で、まとまりが欠けて見えても同じ)
pub fn already_queued(group: &Group, jobs: &[Job]) -> bool {
    use super::super::super::native::same_quake;
    group.quakes.iter().any(|q| {
        jobs.iter()
            .flat_map(|j| &j.quakes)
            .any(|o| same_quake(&q.place(), &o.place()))
    })
}

/// 記録の報から、新しく積むもの: 閉じたまとまりのうち、まだキューに無いもの
pub fn new_jobs(events: &[Event], rules: &Rules, now_ms: u64, existing: &[Job]) -> Vec<Job> {
    detect::groups(&detect::quakes(events, rules), rules)
        .iter()
        .filter(|g| g.is_closed(now_ms, rules) && !already_queued(g, existing))
        .map(|g| Job::new(g, now_ms))
        .collect()
}

fn path_of(dir: &Path, id: &str) -> std::path::PathBuf {
    dir.join(format!("{id}.json"))
}

/// キューの全部を、古い順 (動画の範囲の始まり) に読む。壊れたファイルは飛ばして警告する
pub fn load_all(dir: &Path) -> anyhow::Result<Vec<Job>> {
    let mut jobs: Vec<Job> = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(jobs),
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let job = std::fs::read(&path)
            .map_err(anyhow::Error::from)
            .and_then(|b| serde_json::from_slice::<Job>(&b).map_err(Into::into));
        match job {
            Ok(j) => jobs.push(j),
            Err(e) => tracing::warn!(path = %path.display(), "replay-worker: キューのファイルを読めません: {e:#}"),
        }
    }
    jobs.sort_by_key(|j| (j.from_ms, j.created_ms));
    Ok(jobs)
}

/// 途中まで書いたファイルを読ませないよう、隣に書いてから置き換える
pub fn save(dir: &Path, job: &Job) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = path_of(dir, &job.id);
    let tmp = dir.join(format!("{}.json.tmp", job.id));
    std::fs::write(&tmp, serde_json::to_vec_pretty(job)?).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("replacing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::super::super::testkit::*;
    use super::super::config::WorkerConfig;
    use super::super::detect::{groups, Rules};
    use super::*;

    const POLICY: Policy = Policy {
        max_retries: 5,
        retry_wait_ms: 600_000,
    };

    fn quake(origin_ms: i64, at: (f64, f64)) -> Quake {
        Quake {
            origin_ms,
            lat: Some(at.0),
            lon: Some(at.1),
            max_scale: 30,
            warning: false,
            name: "x".into(),
            last_recv_ms: origin_ms as u64 + 60_000,
            magnitude: None,
            depth_km: None,
        }
    }

    fn job(origin_ms: i64, at: (f64, f64)) -> Job {
        let rules = Rules::from(&WorkerConfig::default());
        Job::new(&groups(&[quake(origin_ms, at)], &rules)[0], 1_000)
    }

    fn done_job(origin_ms: i64) -> Job {
        finish(
            job(origin_ms, TOKYO),
            Outcome::Made {
                video: "a.mp4".into(),
                chapters: vec![],
            },
            9,
            &POLICY,
        )
    }

    #[test]
    fn only_a_made_unuploaded_video_is_next_to_upload_and_never_twice() {
        let waiting = job(T0, TOKYO);
        let older = done_job(T0 - 1_000_000);
        let newer = done_job(T0);
        assert_eq!(next_upload(std::slice::from_ref(&waiting)).map(|j| j.id.clone()), None);
        let jobs = [newer.clone(), waiting, older.clone()];
        assert_eq!(next_upload(&jobs).map(|j| j.id.clone()), Some(older.id.clone()));
        // 上げたら、もう選ばれない (二重に上げない)。ID と時刻が残る
        let up = uploaded(older, "VID1".into(), 77);
        assert_eq!(
            (up.state, up.youtube_id.as_deref(), up.uploaded_ms),
            (State::Uploaded, Some("VID1"), Some(77))
        );
        let jobs = [newer.clone(), up];
        assert_eq!(next_upload(&jobs).map(|j| j.id.clone()), Some(newer.id));
    }

    #[test]
    fn a_repeatedly_rejected_upload_is_given_up() {
        let mut j = done_job(T0);
        for i in 1..=MAX_UPLOAD_FAILURES {
            assert!(next_upload(std::slice::from_ref(&j)).is_some());
            j = upload_rejected(j, "HTTP 400", i as u64);
            assert_eq!(j.upload_failures, i);
        }
        assert!(next_upload(std::slice::from_ref(&j)).is_none());
        assert!(j.error.as_deref().unwrap().contains("HTTP 400"));
        assert_eq!(j.state, State::Done, "動画は done/ に残る");
    }

    #[test]
    fn an_old_queue_file_without_the_upload_fields_still_parses() {
        let j = done_job(T0);
        let mut v = serde_json::to_value(&j).unwrap();
        for k in ["youtube_id", "uploaded_ms", "upload_failures"] {
            v.as_object_mut().unwrap().remove(k);
        }
        let back: Job = serde_json::from_value(v).unwrap();
        assert_eq!(
            (back.youtube_id, back.uploaded_ms, back.upload_failures),
            (None, None, 0)
        );
    }

    #[test]
    fn a_new_job_waits_and_a_made_one_is_done_with_its_chapters() {
        let j = job(T0, TOKYO);
        assert_eq!((j.state, j.retries, j.video.clone()), (State::Waiting, 0, None));
        let chapters = vec![Chapter {
            video_ms: 0,
            origin_ms: Some(T0),
            name: "x".into(),
            max_scale: 30,
        }];
        let done = finish(
            j,
            Outcome::Made {
                video: "a.mp4".into(),
                chapters: chapters.clone(),
            },
            9,
            &POLICY,
        );
        assert_eq!(
            (done.state, done.video.as_deref(), done.chapters),
            (State::Done, Some("a.mp4"), chapters)
        );
    }

    #[test]
    fn a_failure_waits_and_retries_and_the_sixth_failure_gives_up() {
        let mut j = job(T0, TOKYO);
        for n in 1..=5 {
            j = finish(j, Outcome::Failed("ffmpeg".into()), 1_000_000, &POLICY);
            assert_eq!((j.state, j.retries, j.not_before_ms), (State::Waiting, n, 1_600_000));
        }
        j = finish(j, Outcome::Failed("ffmpeg".into()), 1_000_000, &POLICY);
        assert_eq!(
            (j.state, j.retries, j.error.as_deref()),
            (State::Failed, 6, Some("ffmpeg"))
        );
        // 凍結が長く続いたのも、失敗と同じに数える
        let j = finish(job(T0, TOKYO), Outcome::FrozenTooLong, 5, &POLICY);
        assert_eq!((j.state, j.retries), (State::Waiting, 1));
    }

    #[test]
    fn an_interrupted_job_goes_back_to_waiting_without_counting() {
        let mut j = job(T0, TOKYO);
        j.state = State::Making;
        let j = finish(j, Outcome::Interrupted, 77, &POLICY);
        assert_eq!(
            (j.state, j.retries, j.not_before_ms, j.updated_ms),
            (State::Waiting, 0, 0, 77)
        );
    }

    #[test]
    fn a_job_left_making_is_recovered_to_waiting_and_others_are_kept() {
        let mut j = job(T0, TOKYO);
        j.state = State::Making;
        assert_eq!(recover(j.clone(), 5).state, State::Waiting);
        for s in [State::Done, State::Uploaded, State::Failed, State::Waiting] {
            j.state = s;
            assert_eq!(recover(j.clone(), 5).state, s);
        }
    }

    #[test]
    fn the_next_job_is_the_oldest_waiting_one_past_its_wait() {
        let (a, b, c) = (job(T0, TOKYO), job(T0 + 3_600_000, TOKYO), job(T0 + 7_200_000, TOKYO));
        let mut a = a;
        a.state = State::Done;
        let mut b = b;
        b.not_before_ms = 500;
        let jobs = [a, b, c.clone()];
        // b は待ちの時刻前。a は済み。c が次
        assert_eq!(next_job(&jobs, 100).map(|j| &j.id), Some(&c.id));
        assert_eq!(next_job(&jobs, 500).map(|j| j.from_ms), Some(jobs[1].from_ms));
        assert!(next_job(&[], 0).is_none());
    }

    #[test]
    fn a_group_with_a_quake_already_queued_is_not_queued_again() {
        let rules = Rules::from(&WorkerConfig::default());
        let jobs = [job(T0, TOKYO)];
        let same = &groups(&[quake(T0 + 20_000, TOKYO)], &rules)[0];
        assert!(already_queued(same, &jobs));
        let other_place = &groups(&[quake(T0, OSAKA)], &rules)[0];
        assert!(!already_queued(other_place, &jobs));
        let later = &groups(&[quake(T0 + 3_600_000, TOKYO)], &rules)[0];
        assert!(!already_queued(later, &jobs));
    }

    #[test]
    fn only_closed_groups_not_yet_queued_become_jobs() {
        use crate::quake::Scale;
        let rules = Rules::from(&WorkerConfig::default());
        let t = T0 as u64;
        let events = [
            eew("A", 1, t + 5_000, T0, false, Scale::S4, TOKYO),
            eew("B", 1, t + 7_200_000, T0 + 7_200_000, false, Scale::S4, OSAKA),
        ];
        // 最後の報から 60 分たっていなければ、まだ開いている
        let now = t + 7_200_000 + 59 * 60_000;
        let got = new_jobs(&events, &rules, now, &[]);
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].state, got[0].id.as_str()), (State::Waiting, "20260921-231320"));
        // 全部閉じたら 2 つ。1 つがキューに有れば、残りだけ
        let now = now + 2 * 60_000;
        assert_eq!(new_jobs(&events, &rules, now, &[]).len(), 2);
        assert_eq!(new_jobs(&events, &rules, now, &got).len(), 1);
        // 震度 3 未満は積まない
        let small = [eew("C", 1, t, T0, false, Scale::S2, TOKYO)];
        assert!(new_jobs(&small, &rules, now + 9_999_999, &[]).is_empty());
    }

    #[test]
    fn jobs_round_trip_through_files_and_a_broken_file_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (job(T0 + 3_600_000, TOKYO), job(T0, OSAKA));
        save(dir.path(), &a).unwrap();
        save(dir.path(), &b).unwrap();
        std::fs::write(dir.path().join("broken.json"), "{").unwrap();
        std::fs::write(dir.path().join("note.txt"), "x").unwrap();
        // 古い順
        assert_eq!(load_all(dir.path()).unwrap(), vec![b.clone(), a]);
        // 上書き
        let mut b2 = b;
        b2.state = State::Making;
        save(dir.path(), &b2).unwrap();
        assert_eq!(load_all(dir.path()).unwrap()[0].state, State::Making);
        // まだ無いディレクトリは空
        assert!(load_all(&dir.path().join("none")).unwrap().is_empty());
    }

    #[test]
    fn the_file_is_readable_json_with_plain_state_names() {
        let dir = tempfile::tempdir().unwrap();
        let j = job(T0, TOKYO);
        save(dir.path(), &j).unwrap();
        let text = std::fs::read_to_string(dir.path().join(format!("{}.json", j.id))).unwrap();
        assert!(text.contains("\"state\": \"waiting\""), "{text}");
    }
}
