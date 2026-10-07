//! `POST /api/tts/announce` (公開・認証なし) に載せる報の入力検証。S-01 (docs/security-audit-2026-10-07.md)。
//! 件数と文字列の長さに上限を置き、同じ地域名の重複は 1 つにまとめる (読み上げ内容としても重複は不要)。
//! 上限は実データ (日本の津波予報区 66、震度観測の地域 約 190、都道府県 47、震度観測点 約 4,400) の正当な最大が通る値。

use std::collections::HashSet;

use crate::quake::model::{Event, EventBody};

/// 津波予報区は 66。余裕を見て 100
pub const MAX_TSUNAMI_AREAS: usize = 100;
/// 都道府県は 47。余裕を見て 50
pub const MAX_PREFS: usize = 50;
/// EEW の地域 (細分区域) と地震感知情報の地域。震度観測の地域は約 190。余裕を見て 256
pub const MAX_AREAS: usize = 256;
/// 震度観測点は約 4,400 (デモの記録の最大は 2,829)。読み上げには使わないが本文に載る
pub const MAX_POINTS: usize = 5_000;
/// 地域名・都道府県名・震源名・観測点名の長さ (文字数)。最長の予報区名・観測点名でも 30 文字に届かない
pub const MAX_NAME_CHARS: usize = 64;
/// id・source の長さ
pub const MAX_ID_CHARS: usize = 128;

fn name_ok(s: &str) -> bool {
    s.chars().count() <= MAX_NAME_CHARS
}

/// 同じ名前を最初の 1 つだけ残す (順序は保つ)
fn dedup_by_name<T>(v: Vec<T>, name: impl Fn(&T) -> &str) -> Vec<T> {
    let mut seen = HashSet::new();
    v.into_iter().filter(|x| seen.insert(name(x).to_string())).collect()
}

/// 上限を超えていれば None。そうでなければ読み上げに使う、重複を除いた報を返す。
pub fn sanitize(mut ev: Event) -> Option<Event> {
    if ev.id.chars().count() > MAX_ID_CHARS || ev.source.chars().count() > MAX_ID_CHARS {
        return None;
    }
    match &mut ev.body {
        EventBody::Quake(q) => {
            if q.points.len() > MAX_POINTS
                || q.pref_max.len() > MAX_PREFS
                || !q.hypocenter.as_ref().is_none_or(|h| name_ok(&h.name))
                || !q.pref_max.iter().all(|p| name_ok(&p.pref))
                || !q.points.iter().all(|p| name_ok(&p.pref) && name_ok(&p.addr))
            {
                return None;
            }
            q.pref_max = dedup_by_name(std::mem::take(&mut q.pref_max), |p| &p.pref);
        }
        EventBody::Eew(e) => {
            if e.areas.len() > MAX_AREAS
                || e.pref_max.len() > MAX_PREFS
                || !e.hypocenter.as_ref().is_none_or(|h| name_ok(&h.name))
                || !e.pref_max.iter().all(|p| name_ok(&p.pref))
                || !e.areas.iter().all(|a| name_ok(&a.pref) && name_ok(&a.name))
            {
                return None;
            }
            e.pref_max = dedup_by_name(std::mem::take(&mut e.pref_max), |p| &p.pref);
        }
        EventBody::Tsunami(t) => {
            if t.areas.len() > MAX_TSUNAMI_AREAS || !t.areas.iter().all(|a| name_ok(&a.name)) {
                return None;
            }
            t.areas = dedup_by_name(std::mem::take(&mut t.areas), |a| &a.name);
        }
        EventBody::Userquake(u) => {
            if u.areas.len() > MAX_AREAS {
                return None;
            }
        }
        EventBody::EewDetection(_) => {}
    }
    Some(ev)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_event_in_the_noto_2024_record_passes_unchanged_in_size() {
        let raw = include_str!("../../../../web/public/demo/noto2024.json");
        let v: serde_json::Value = serde_json::from_str(raw).unwrap();
        let events: Vec<Event> = serde_json::from_value(v["events"].clone()).unwrap();
        assert!(!events.is_empty());
        for e in events {
            assert!(sanitize(e.clone()).is_some(), "{}", e.id);
        }
    }
}
