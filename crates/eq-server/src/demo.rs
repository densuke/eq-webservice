//! デモの場面 (samples/scenarios/*.jsonl) を、画面のデモモード用の JSON (web/public/demo/) に変換する。
//! 変換はサーバと同じ処理を使うので、デモと本番で表示が食い違わない。
//!
//! 場面のファイルは P2P地震情報 / Wolfx 形式の JSON Lines。先頭のコメントで名前と説明を書く:
//!   # name: 標準: 宮城県沖の緊急地震速報 (警報)
//!   # description: 緊急地震速報 (警報) → 震度速報 → ...
//!   # source: 出典 (過去の地震の記録を再生する場面だけ)

use std::path::Path;

use anyhow::Context;
use eq_core::Event;
use serde::Serialize;

use crate::source::replay;

/// 一覧で先頭に出す場面
const FIRST: &str = "standard";

#[derive(Serialize)]
struct Summary {
    id: String,
    name: String,
    description: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    source: String,
}

#[derive(Serialize)]
struct Scenario {
    #[serde(flatten)]
    summary: Summary,
    events: Vec<Event>,
}

fn scenario(id: &str, text: &str) -> anyhow::Result<Scenario> {
    let header = |key: &str| {
        text.lines()
            .filter_map(|l| l.strip_prefix('#'))
            .find_map(|l| l.trim().strip_prefix(key).map(|v| v.trim().to_string()))
            .unwrap_or_default()
    };
    Ok(Scenario {
        summary: Summary {
            id: id.to_string(),
            name: Some(header("name:"))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| id.to_string()),
            description: header("description:"),
            source: header("source:"),
        },
        events: replay::load(text)?,
    })
}

/// src の *.jsonl をすべて変換し、out/<id>.json と一覧 out/index.json を書く
pub fn convert_dir(src: &Path, out: &Path) -> anyhow::Result<()> {
    let mut files: Vec<_> = std::fs::read_dir(src)
        .with_context(|| format!("reading {}", src.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .collect();
    files.sort_by_key(|p| {
        let stem = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
        (stem != FIRST, stem)
    });
    std::fs::create_dir_all(out)?;
    let mut index = Vec::new();
    for path in files {
        let id = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let s = scenario(&id, &text).with_context(|| format!("converting {}", path.display()))?;
        // 場面の本体は大きくなる (観測点が数千ある) ので詰めて書く
        std::fs::write(out.join(format!("{id}.json")), serde_json::to_string(&s)? + "\n")?;
        index.push(s.summary);
    }
    std::fs::write(out.join("index.json"), serde_json::to_string_pretty(&index)? + "\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// リポジトリに入れた変換済みのファイルが、場面のファイルと食い違っていないか
    #[test]
    fn committed_demo_files_are_up_to_date() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let out = tempfile::tempdir().unwrap();
        convert_dir(&root.join("samples/scenarios"), out.path()).unwrap();
        for entry in std::fs::read_dir(out.path()).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            let committed = std::fs::read_to_string(root.join("web/public/demo").join(&name)).unwrap_or_default();
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                committed,
                "web/public/demo/{name} が古い。`cargo run -p eq-server -- convert samples/scenarios web/public/demo` で作り直す"
            );
        }
    }

    #[test]
    fn reads_name_and_description_from_header() {
        let s = scenario("x", "# name: 名前\n# description: 説明\n# source: 出典\n").unwrap();
        assert_eq!(
            (
                s.summary.name.as_str(),
                s.summary.description.as_str(),
                s.summary.source.as_str()
            ),
            ("名前", "説明", "出典")
        );
        assert_eq!(scenario("x", "").unwrap().summary.name, "x");
    }
}
