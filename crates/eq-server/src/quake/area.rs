//! 地震情報細分区域 ("茨城県南部", "石狩地方北部" など) から都道府県を求める。

const PREFS: [&str; 47] = [
    "北海道",
    "青森県",
    "岩手県",
    "宮城県",
    "秋田県",
    "山形県",
    "福島県",
    "茨城県",
    "栃木県",
    "群馬県",
    "埼玉県",
    "千葉県",
    "東京都",
    "神奈川県",
    "新潟県",
    "富山県",
    "石川県",
    "福井県",
    "山梨県",
    "長野県",
    "岐阜県",
    "静岡県",
    "愛知県",
    "三重県",
    "滋賀県",
    "京都府",
    "大阪府",
    "兵庫県",
    "奈良県",
    "和歌山県",
    "鳥取県",
    "島根県",
    "岡山県",
    "広島県",
    "山口県",
    "徳島県",
    "香川県",
    "愛媛県",
    "高知県",
    "福岡県",
    "佐賀県",
    "長崎県",
    "熊本県",
    "大分県",
    "宮崎県",
    "鹿児島県",
    "沖縄県",
];

/// 都道府県名で始まらない地震情報細分区域 (気象庁「地震情報／細分区域」GIS データで確認)
const HOKKAIDO_REGIONS: [&str; 16] = [
    "石狩", "渡島", "檜山", "後志", "空知", "上川", "留萌", "宗谷", "網走", "北見", "紋別", "胆振", "日高", "十勝",
    "釧路", "根室",
];
const TOKYO_ISLANDS: [&str; 6] = ["神津島", "伊豆大島", "新島", "三宅島", "八丈島", "小笠原"];

/// 細分区域名 ("茨城県南部", "石狩地方北部" など) の都道府県。分からなければ空文字
pub fn area_pref(area: &str) -> &'static str {
    if let Some(p) = PREFS.iter().find(|p| area.starts_with(*p)) {
        return p;
    }
    if HOKKAIDO_REGIONS
        .iter()
        .any(|r| area.starts_with(r) && area[r.len()..].starts_with("地方"))
    {
        return "北海道";
    }
    if TOKYO_ISLANDS.iter().any(|i| area.starts_with(i)) {
        return "東京都";
    }
    ""
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_areas_to_prefectures() {
        assert_eq!(area_pref("茨城県南部"), "茨城県");
        assert_eq!(area_pref("東京都２３区"), "東京都");
        assert_eq!(area_pref("石狩地方北部"), "北海道");
        assert_eq!(area_pref("網走地方"), "北海道");
        assert_eq!(area_pref("八丈島"), "東京都");
        assert_eq!(area_pref("伊豆大島"), "東京都");
        assert_eq!(area_pref("国後島"), "");
    }
}
