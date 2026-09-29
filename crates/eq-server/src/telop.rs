//! テロップ (画面上部で平常時に切り替えて表示する文)。出典と注意書きに、設定の文を足す。

use std::sync::Arc;

use axum::routing::get;
use axum::{Json, Router};

/// 表示する文。eew_enabled は Wolfx から緊急地震速報を受けているか (出典に Wolfx を含める)
pub fn messages(extra: &[String], eew_enabled: bool) -> Vec<String> {
    let mut out = vec!["地震情報: P2P地震情報 (気象庁発表の情報)".to_string()];
    if eew_enabled {
        out.push("緊急地震速報 (予報): Wolfx Project 経由 (気象庁発表の情報、非公式の中継)".into());
    }
    out.push(
        "地図: 地球地図日本 (国土地理院) を加工 / 津波予報区・細分区域・震度観測点・警報の区域: 気象庁のデータを加工"
            .into(),
    );
    out.push("気象警報・注意報: 気象庁防災情報XML / 揺れの報告 (地震感知情報): P2P地震情報".into());
    out.push("この画面は公式の警報ではありません。気象庁・自治体の情報を確認してください".into());
    out.extend(extra.iter().filter(|s| !s.trim().is_empty()).cloned());
    out
}

/// `GET /api/telop` : 文の配列 (JSON)
pub fn router(messages: Vec<String>) -> Router {
    let messages = Arc::new(messages);
    Router::new().route(
        "/api/telop",
        get(move || async move { Json(messages.as_ref().clone()) }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wolfx_credit_only_when_enabled_and_extra_appended() {
        let off = messages(&[], false);
        assert!(!off.iter().any(|m| m.contains("Wolfx")));
        let on = messages(&["保守のお知らせ".into(), " ".into()], true);
        assert!(on.iter().any(|m| m.contains("Wolfx")));
        assert_eq!(on.last().unwrap(), "保守のお知らせ");
        assert_eq!(on.len(), off.len() + 2);
    }
}
