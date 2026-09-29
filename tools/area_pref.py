"""地震情報細分区域の名前から都道府県を求める (crates/eq-core/src/area.rs と同じ)。"""

# stations.json の pref は 1 始まりの都道府県番号
PREFS = (
    "北海道 青森県 岩手県 宮城県 秋田県 山形県 福島県 茨城県 栃木県 群馬県 埼玉県 千葉県 東京都 神奈川県 "
    "新潟県 富山県 石川県 福井県 山梨県 長野県 岐阜県 静岡県 愛知県 三重県 滋賀県 京都府 大阪府 兵庫県 "
    "奈良県 和歌山県 鳥取県 島根県 岡山県 広島県 山口県 徳島県 香川県 愛媛県 高知県 福岡県 佐賀県 長崎県 "
    "熊本県 大分県 宮崎県 鹿児島県 沖縄県"
).split()
# 都道府県名で始まらない細分区域 (crates/eq-core/src/area.rs と同じ)
HOKKAIDO_REGIONS = "石狩 渡島 檜山 後志 空知 上川 留萌 宗谷 網走 北見 紋別 胆振 日高 十勝 釧路 根室".split()
TOKYO_ISLANDS = "神津島 伊豆大島 新島 三宅島 八丈島 小笠原".split()


def area_pref(area):
    for p in PREFS:
        if area.startswith(p):
            return p
    if any(area.startswith(r) and area[len(r):].startswith("地方") for r in HOKKAIDO_REGIONS):
        return "北海道"
    if any(area.startswith(i) for i in TOKYO_ISLANDS):
        return "東京都"
    return ""
