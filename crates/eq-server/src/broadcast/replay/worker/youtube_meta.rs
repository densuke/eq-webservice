//! YouTube に上げる動画のタイトル・説明文 (docs/replay-video.md 5.3)。キューの 1 つのまとまりから作る純粋な関数。

use super::super::plan::Chapter;
use super::detect::Quake;
use crate::quake::{jst, Scale};

/// YouTube の上限: タイトル 100 文字、説明文 5000 バイト
const TITLE_MAX_CHARS: usize = 100;
const DESCRIPTION_MAX_BYTES: usize = 4900;
/// 説明文に載せる地震の一覧の行数の上限 (長い連続地震で説明文があふれないように)
const LIST_MAX: usize = 30;
/// チャプターの条件 (YouTube): 3 個以上、各 10 秒以上
const CHAPTER_MIN_COUNT: usize = 3;
const CHAPTER_MIN_MS: u64 = 10_000;

/// 画面の出典 (broadcast/native/panel.rs の CREDIT) と同じもの
const CREDITS: [&str; 5] = [
    "情報: P2P地震情報 (気象庁発表)",
    "地図: 地球地図日本 (国土地理院) を加工",
    "津波予報区・細分区域・震度観測点: 気象庁のデータを加工",
    "天気・アメダス: 気象庁",
    "天気アイコン: 気象庁ホームページを加工 (https://www.jma.go.jp/bosai/forecast/)",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoMeta {
    pub title: String,
    pub description: String,
}

/// YouTube が受け付けない文字 (< と >) を取る
fn clean(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, '<' | '>')).collect()
}

fn place(q: &Quake) -> String {
    if q.name.is_empty() {
        "震源不明".to_string()
    } else {
        clean(&q.name)
    }
}

/// "震度3" (震度が分からなければ None)
fn scale_text(max_scale: i32) -> Option<String> {
    let s = Scale(max_scale);
    s.is_known().then(|| format!("震度{}", s.label()))
}

fn jst_minute(ms: i64) -> String {
    // "YYYY/MM/DD HH:MM:SS" の秒を落とす
    jst::format(ms)[..16].to_string()
}

fn clock(ms: i64) -> String {
    jst::format(ms)[11..16].to_string()
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max - 1).collect();
    t.push('…');
    t
}

pub fn title(quakes: &[Quake]) -> String {
    let Some(first) = quakes.first() else {
        return "【地震の記録】(再現)".to_string();
    };
    let max = quakes.iter().map(|q| q.max_scale).max().unwrap_or(0);
    let scale = scale_text(max).map(|s| format!(" 最大{s}")).unwrap_or_default();
    let t = if quakes.len() == 1 {
        format!(
            "【地震の記録】{} {}{scale} (再現)",
            jst_minute(first.origin_ms),
            place(first)
        )
    } else {
        format!(
            "【連続して発生した地震の記録】{} {}ほか計{}回{scale} (再現)",
            jst_minute(first.origin_ms),
            place(first),
            quakes.len()
        )
    };
    truncate_chars(&t, TITLE_MAX_CHARS)
}

/// "0:00"・"12:05"・"1:02:03"
pub fn timestamp(video_ms: u64) -> String {
    let s = video_ms / 1000;
    match s {
        0..3600 => format!("{}:{:02}", s / 60, s % 60),
        _ => format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60),
    }
}

/// YouTube がチャプターとして扱う条件を満たすか: 先頭が 0:00、3 個以上、各 10 秒以上 (増えていく順)
pub fn chapters_valid(chapters: &[Chapter]) -> bool {
    chapters.len() >= CHAPTER_MIN_COUNT
        && chapters[0].video_ms == 0
        && chapters
            .windows(2)
            .all(|w| w[1].video_ms >= w[0].video_ms + CHAPTER_MIN_MS)
}

/// チャプターの行 (条件を満たさないときは None。説明文には付けない)
pub fn chapter_lines(chapters: &[Chapter]) -> Option<Vec<String>> {
    chapters_valid(chapters).then(|| {
        chapters
            .iter()
            .map(|c| {
                let when = c.origin_ms.map(|ms| format!("{} ", clock(ms))).unwrap_or_default();
                let scale = scale_text(c.max_scale).map(|s| format!(" {s}")).unwrap_or_default();
                let name = if c.name.is_empty() {
                    "震源不明".to_string()
                } else {
                    clean(&c.name)
                };
                format!("{} {when}{name}{scale}", timestamp(c.video_ms))
            })
            .collect()
    })
}

fn quake_line(q: &Quake) -> String {
    let mut parts = vec![format!("{} {}", jst_minute(q.origin_ms), place(q))];
    if let Some(m) = q.magnitude {
        parts.push(format!("M{m:.1}"));
    }
    match q.depth_km {
        Some(0) => parts.push("ごく浅い".into()),
        Some(d) => parts.push(format!("深さ{d}km")),
        None => {}
    }
    if let Some(s) = scale_text(q.max_scale) {
        parts.push(format!("最大{s}"));
    }
    parts.join(" ")
}

pub fn description(quakes: &[Quake], chapters: &[Chapter]) -> String {
    let mut out: Vec<String> = vec![
        "気象庁などが発表した地震の記録 (P2P地震情報) から、当時の画面を描き直した再現映像です。実際の放送・配信ではありません。".into(),
        "時刻は日本時間です。".into(),
    ];
    if let Some(lines) = chapter_lines(chapters) {
        out.push(String::new());
        out.push("チャプター".into());
        out.extend(lines);
    }
    out.push(String::new());
    out.push("地震の一覧".into());
    out.extend(quakes.iter().take(LIST_MAX).map(quake_line));
    if quakes.len() > LIST_MAX {
        out.push(format!("ほか {} 件", quakes.len() - LIST_MAX));
    }
    out.push(String::new());
    out.push("出典".into());
    out.extend(CREDITS.iter().map(|c| c.to_string()));
    let text = out.join("\n");
    if text.len() <= DESCRIPTION_MAX_BYTES {
        return text;
    }
    // 上限を超えるなら、文字の途中で切らずに丸める (出典が欠けないよう、通常は到達しない)
    let mut end = DESCRIPTION_MAX_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

pub fn build(quakes: &[Quake], chapters: &[Chapter]) -> VideoMeta {
    VideoMeta {
        title: title(quakes),
        description: description(quakes, chapters),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t() -> i64 {
        jst::parse_ms("2026/10/02 17:39:00").unwrap()
    }

    fn quake(off_min: i64, name: &str, scale: i32) -> Quake {
        Quake {
            origin_ms: t() + off_min * 60_000,
            lat: None,
            lon: None,
            max_scale: scale,
            warning: false,
            name: name.into(),
            last_recv_ms: 0,
            magnitude: Some(4.5),
            depth_km: Some(10),
        }
    }

    fn chapter(video_ms: u64, off_min: i64, name: &str, scale: i32) -> Chapter {
        Chapter {
            video_ms,
            origin_ms: Some(t() + off_min * 60_000),
            name: name.into(),
            max_scale: scale,
        }
    }

    #[test]
    fn a_single_quake_title_has_time_place_and_scale() {
        assert_eq!(
            title(&[quake(0, "熊本県熊本地方", 30)]),
            "【地震の記録】2026/10/02 17:39 熊本県熊本地方 最大震度3 (再現)"
        );
    }

    #[test]
    fn a_series_title_has_the_first_place_the_count_and_the_biggest_scale() {
        let q = [
            quake(0, "与那国島近海", 30),
            quake(5, "与那国島近海", 45),
            quake(9, "台湾付近", 20),
        ];
        assert_eq!(
            title(&q),
            "【連続して発生した地震の記録】2026/10/02 17:39 与那国島近海ほか計3回 最大震度5弱 (再現)"
        );
    }

    #[test]
    fn the_title_never_exceeds_the_youtube_limit_and_drops_angle_brackets() {
        let long = "あ".repeat(200);
        assert_eq!(title(&[quake(0, &long, 30)]).chars().count(), TITLE_MAX_CHARS);
        assert!(!title(&[quake(0, "<b>x</b>", 30)]).contains(['<', '>']));
    }

    #[test]
    fn an_unknown_place_and_scale_are_still_titled() {
        assert_eq!(
            title(&[quake(0, "", -1)]),
            "【地震の記録】2026/10/02 17:39 震源不明 (再現)"
        );
    }

    #[test]
    fn timestamps_use_the_youtube_format() {
        assert_eq!(timestamp(0), "0:00");
        assert_eq!(timestamp(65_000), "1:05");
        assert_eq!(timestamp(3_723_000), "1:02:03");
    }

    #[test]
    fn valid_chapters_become_lines() {
        let c = [
            chapter(0, 0, "A地方", 30),
            chapter(40_000, 5, "B地方", 45),
            chapter(95_000, 9, "C地方", 20),
        ];
        assert_eq!(
            chapter_lines(&c).unwrap(),
            [
                "0:00 17:39 A地方 震度3",
                "0:40 17:44 B地方 震度5弱",
                "1:35 17:48 C地方 震度2"
            ]
        );
    }

    #[test]
    fn invalid_chapter_lists_are_omitted() {
        let ok = [
            chapter(0, 0, "A", 30),
            chapter(20_000, 1, "B", 30),
            chapter(40_000, 2, "C", 30),
        ];
        assert!(chapters_valid(&ok));
        // 2 個だけ
        assert!(!chapters_valid(&ok[..2]));
        // 先頭が 0:00 でない
        let late = [chapter(1_000, 0, "A", 30), ok[1].clone(), ok[2].clone()];
        assert!(!chapters_valid(&late));
        // 10 秒に満たない間
        let short = [ok[0].clone(), chapter(9_999, 1, "B", 30), chapter(30_000, 2, "C", 30)];
        assert!(!chapters_valid(&short));
        // 逆順
        let back = [ok[0].clone(), ok[2].clone(), ok[1].clone()];
        assert!(!chapters_valid(&back));
        assert!(chapter_lines(&short).is_none());
        assert!(!description(&[quake(0, "A", 30)], &short).contains("チャプター"));
    }

    #[test]
    fn the_description_has_the_note_the_list_the_chapters_and_the_credits() {
        let q = [quake(0, "A地方", 30), quake(5, "B地方", 45), quake(9, "C地方", 20)];
        let c = [
            chapter(0, 0, "A地方", 30),
            chapter(40_000, 5, "B地方", 45),
            chapter(95_000, 9, "C地方", 20),
        ];
        let d = description(&q, &c);
        assert!(d.contains("再現映像です"));
        assert!(d.contains("チャプター\n0:00 17:39 A地方 震度3\n"));
        assert!(d.contains("2026/10/02 17:39 A地方 M4.5 深さ10km 最大震度3"));
        assert!(d.contains("情報: P2P地震情報 (気象庁発表)"));
        assert!(d.contains("天気アイコン: 気象庁ホームページを加工"));
        // チャプターが先に来る (YouTube は説明文の中の時刻の行を拾う)
        assert!(d.find("チャプター").unwrap() < d.find("地震の一覧").unwrap());
    }

    #[test]
    fn a_long_series_list_is_capped_and_the_description_stays_under_the_limit() {
        let q: Vec<Quake> = (0..200).map(|i| quake(i, "とても長い名前の震源地域", 30)).collect();
        let d = description(&q, &[]);
        assert!(d.contains("ほか 170 件"));
        assert!(d.len() <= DESCRIPTION_MAX_BYTES);
        assert!(d.contains("出典"));
    }
}
