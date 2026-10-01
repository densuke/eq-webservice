//! `eq-server replay-video` の引数。

use std::path::PathBuf;

use anyhow::Context;

use super::super::native::Place;
use super::super::BroadcastConfig;

pub const USAGE: &str = "\
usage: eq-server replay-video --from <ms> --to <ms> --out <x.mp4> (--events <jsonl> | --archive <URL>)
           [--quake <origin_ms>[,<lat>,<lon>]]... [--chapters <x.json>]
           [--fps 5] [--no-zoom] [--label \"記録から再現\"] [--map-dir web/public] [--font <ttf/ttc>] [--ffmpeg ffmpeg]
  --from/--to   報を集める範囲 (received_at_ms。epoch ミリ秒)。--archive のときは 1 時間まで
  --events      eq-server の jsonl の記録 (sink が書いたもの) を直接読む。samples/scenarios の形も読める
  --archive     /api/archive から取る (例: https://eq.fuga.jp)
  --quake       動画に入れる地震 (発生時刻と、分かれば震源の緯度・経度)。何度でも書ける (連続地震)。省けば範囲で最大震度の 1 つ
  --chapters    各地震の始まりの、動画の中の時刻を JSON で書き出す
  --no-zoom     震源へ寄らず、常に日本全体を映す (既定は web と同じく震源へ寄る)";

/// 報の取り方
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Events(PathBuf),
    Archive(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub from: u64,
    pub to: u64,
    pub out: PathBuf,
    pub source: Source,
    /// 動画に入れる地震。空なら範囲で最大震度の 1 つ
    pub quakes: Vec<Place>,
    pub chapters: Option<PathBuf>,
    pub fps: u32,
    /// 地震のとき震源へ寄る
    pub zoom: bool,
    pub label: String,
    pub map_dir: String,
    pub font: String,
    pub ffmpeg: String,
}

pub fn parse(args: &[String]) -> anyhow::Result<Options> {
    let d = BroadcastConfig::default();
    let (mut from, mut to, mut out) = (None, None, None);
    let (mut events, mut archive) = (None, None);
    let mut o = Options {
        from: 0,
        to: 0,
        out: PathBuf::new(),
        source: Source::Events(PathBuf::new()),
        quakes: Vec::new(),
        chapters: None,
        fps: 5,
        zoom: true,
        label: "記録から再現".into(),
        map_dir: d.map_dir,
        font: d.font,
        ffmpeg: d.ffmpeg,
    };
    let mut it = args.iter();
    while let Some(key) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .with_context(|| format!("{name} needs a value\n\n{USAGE}"))
        };
        match key.as_str() {
            "--from" => from = Some(ms(&value("--from")?)?),
            "--to" => to = Some(ms(&value("--to")?)?),
            "--out" => out = Some(PathBuf::from(value("--out")?)),
            "--events" => events = Some(PathBuf::from(value("--events")?)),
            "--archive" => archive = Some(value("--archive")?.trim_end_matches('/').to_string()),
            "--quake" => o.quakes.push(quake(&value("--quake")?)?),
            "--chapters" => o.chapters = Some(PathBuf::from(value("--chapters")?)),
            "--fps" => o.fps = value("--fps")?.parse().context("--fps")?,
            "--no-zoom" => o.zoom = false,
            "--label" => o.label = value("--label")?,
            "--map-dir" => o.map_dir = value("--map-dir")?,
            "--font" => o.font = value("--font")?,
            "--ffmpeg" => o.ffmpeg = value("--ffmpeg")?,
            other => anyhow::bail!("unknown argument {other:?}\n\n{USAGE}"),
        }
    }
    o.from = from.with_context(|| format!("--from is required\n\n{USAGE}"))?;
    o.to = to.with_context(|| format!("--to is required\n\n{USAGE}"))?;
    o.out = out.with_context(|| format!("--out is required\n\n{USAGE}"))?;
    anyhow::ensure!(o.from <= o.to, "--from must not be later than --to");
    anyhow::ensure!((1..=60).contains(&o.fps), "--fps must be 1..=60");
    o.source = match (events, archive) {
        (Some(p), None) => Source::Events(p),
        (None, Some(u)) => Source::Archive(u),
        _ => anyhow::bail!("give exactly one of --events and --archive\n\n{USAGE}"),
    };
    Ok(o)
}

/// `<origin_ms>` か `<origin_ms>,<lat>,<lon>`
fn quake(s: &str) -> anyhow::Result<Place> {
    let mut it = s.split(',');
    let origin = it.next().and_then(|v| v.parse::<i64>().ok());
    let place = |v: Option<&str>| {
        v.map(|v| v.parse::<f64>().with_context(|| format!("--quake {s:?}")))
            .transpose()
    };
    let (lat, lon) = (place(it.next())?, place(it.next())?);
    anyhow::ensure!(
        origin.is_some() && lat.is_some() == lon.is_some() && it.next().is_none(),
        "--quake は <origin_ms> か <origin_ms>,<lat>,<lon>: {s:?}"
    );
    Ok(Place {
        origin_ms: origin,
        lat,
        lon,
    })
}

fn ms(s: &str) -> anyhow::Result<u64> {
    s.parse()
        .with_context(|| format!("not a number of milliseconds: {s:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(a: &[&str]) -> anyhow::Result<Options> {
        parse(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn reads_the_required_options_and_defaults_the_rest() {
        let o = parse_strs(&[
            "--from",
            "1",
            "--to",
            "9",
            "--out",
            "x.mp4",
            "--archive",
            "https://eq.fuga.jp/",
        ])
        .unwrap();
        assert_eq!((o.from, o.to, o.fps), (1, 9, 5));
        assert_eq!(o.source, Source::Archive("https://eq.fuga.jp".into()));
        assert_eq!(o.label, "記録から再現");
        let o = parse_strs(&[
            "--from", "1", "--to", "9", "--out", "x", "--events", "e.jsonl", "--fps", "10",
        ])
        .unwrap();
        assert_eq!((o.source, o.fps), (Source::Events("e.jsonl".into()), 10));
    }

    #[test]
    fn the_view_zooms_in_on_the_epicenter_unless_told_not_to() {
        let base = ["--from", "1", "--to", "9", "--out", "x", "--events", "e"];
        assert!(parse_strs(&base).unwrap().zoom);
        assert!(!parse_strs(&[&base[..], &["--no-zoom"]].concat()).unwrap().zoom);
    }

    #[test]
    fn quakes_are_repeatable_and_may_omit_the_epicenter() {
        let o = parse_strs(&[
            "--from",
            "1",
            "--to",
            "9",
            "--out",
            "x",
            "--events",
            "e",
            "--quake",
            "1790000000000,35.5,139.7",
            "--quake",
            "5",
            "--chapters",
            "c.json",
        ])
        .unwrap();
        assert_eq!(o.quakes.len(), 2);
        assert_eq!(
            (o.quakes[0].origin_ms, o.quakes[0].lat, o.quakes[0].lon),
            (Some(1_790_000_000_000), Some(35.5), Some(139.7))
        );
        assert_eq!((o.quakes[1].origin_ms, o.quakes[1].lat), (Some(5), None));
        assert_eq!(o.chapters, Some("c.json".into()));
        let bad = |q: &str| parse_strs(&["--from", "1", "--to", "9", "--out", "x", "--events", "e", "--quake", q]);
        assert!(bad("x").is_err() && bad("1,2").is_err() && bad("1,2,3,4").is_err());
    }

    #[test]
    fn refuses_a_missing_or_double_source_and_bad_values() {
        let base = ["--from", "1", "--to", "9", "--out", "x"];
        assert!(parse_strs(&base).is_err());
        assert!(parse_strs(&[&base[..], &["--events", "a", "--archive", "b"]].concat()).is_err());
        assert!(parse_strs(&["--to", "9", "--out", "x", "--events", "a"]).is_err());
        assert!(parse_strs(&["--from", "9", "--to", "1", "--out", "x", "--events", "a"]).is_err());
        assert!(parse_strs(&[&base[..], &["--events", "a", "--fps", "0"]].concat()).is_err());
        assert!(parse_strs(&[&base[..], &["--events", "a", "--nope", "1"]].concat()).is_err());
    }
}
