// レイアウトの設計システム案 (提案・ドラフト) のデータ。比率の図は layout-app.js が組み立てる。
// 「実測」= docs/ui-spec/measured.js (Chromium) / 「概算」= 共有されたスクリーンショットからの目測 (±3%程度)。
// 領域の座標は、キャンバスに対する割合 (%)。[id, 表示名, 種別, x, y, w, h]

window.LS_PROFILES = [
  {
    id: "regular", label: "regular (PC・タブレット横)", basis: "実測 1280×720 (この図。気象警報の帯 31px を除いて換算)。1440×900 では 73.6 : 26.4", ratio: "地図 : 右パネル ≒ 70 : 30 (1280 のとき。右は 380px 固定なので、広いほど地図の割合が増える)", canvas: [1280, 720],
    boxes: [
      ["topbar", "上部バー", "bar", 0, 0, 100, 5.1],
      ["map", "地図 (枠)", "map", 0, 5.1, 70.3, 94.9],
      ["inset", "別枠 (南西諸島)", "overlay", 0.8, 6.5, 17.7, 30.6],
      ["legend", "凡例", "overlay", 0.8, 65, 4.2, 33],
      ["clock", "時計", "overlay", 55.4, 86, 14.1, 12.6],
      ["detail", "詳細", "panel", 70.3, 5.1, 29.7, 10],
      ["list", "履歴 (見出し込み)", "panel", 70.3, 15.1, 29.7, 34.4],
      ["notice", "お知らせ", "panel", 70.3, 49.5, 29.7, 36.5],
      ["credit", "出典", "panel", 70.3, 86, 29.7, 14],
    ],
  },
  {
    id: "compact", label: "compact (スマホ縦)", basis: "実測 390×844 (ページ長 876)。履歴 (#list) は空のとき 0px、図の「履歴」は見出し行", ratio: "縦に積む。地図 ≒ 58% (60svh) / 詳細・履歴・お知らせ・出典 ≒ 28%。ページ全体がスクロール", canvas: [390, 876],
    boxes: [
      ["topbar", "上部バー (折り返し・sticky)", "bar", 0, 0, 100, 10.4],
      ["warn", "気象警報の帯", "bar", 0, 10.4, 100, 3.5],
      ["map", "地図 (60svh)", "map", 0, 13.9, 100, 57.8],
      ["inset", "別枠 (小)", "overlay", 2.6, 15, 36.9, 16],
      ["clock", "時計", "overlay", 60, 62.6, 38.5, 8.5],
      ["legend", "凡例 (左・縮小)", "overlay", 2.6, 32, 12.8, 22.6],
      ["detail", "詳細", "panel", 0, 71.7, 100, 8.4],
      ["list", "履歴", "panel", 0, 80.1, 100, 3.7],
      ["notice", "お知らせ", "panel", 0, 83.8, 100, 4.8],
      ["credit", "出典", "panel", 0, 88.6, 100, 11.4],
    ],
  },
  {
    id: "native", label: "broadcast-native (現行・1280×720 固定)", basis: "Rust の定数 (draw.rs / frame.rs / panel.rs / notice.rs)。右の列の履歴・お知らせ・出典の高さは推定", ratio: "地図 : 右パネル = 70.3 : 29.7 (900 : 380)。1280×720 固定で、比率ではなく px の定数で持つ。このまま 1 つの「固定サイズのプロファイル」として宣言できる", canvas: [1280, 720],
    boxes: [
      ["topbar", "上部バー (36px)", "bar", 0, 0, 100, 5],
      ["map", "地図", "map", 0, 5, 70.3, 95],
      ["inset", "別枠 (南西諸島 220px)", "overlay", 0.8, 6.4, 17.7, 30.6],
      ["warnlegend", "凡例 (警報・平時のみ)", "overlay", 0.8, 66.5, 5.2, 10.8],
      ["legend", "凡例 (震度)", "overlay", 0.8, 78.2, 2.7, 20.4],
      ["clock", "時計", "overlay", 55.8, 88.3, 13.8, 10.3],
      ["detail", "詳細", "panel", 70.3, 5, 29.7, 20],
      ["list", "履歴 (5 件)", "panel", 70.3, 25.1, 29.7, 41.6],
      ["notice", "お知らせ", "panel", 70.3, 68.3, 29.7, 14.4],
      ["credit", "出典", "panel", 70.3, 87.5, 29.7, 12.5],
    ],
  },
  {
    id: "jquake", label: "参照: JQuake 型 (2 カラム + 右上に寄り図)", basis: "概算 (スクリーンショット 2000×1150)", ratio: "左の全国図 : 右の列 ≒ 68 : 32。右の列は 寄り図 ≒ 41% : 履歴 ≒ 59%。うちの現行 native (70 : 30) とほぼ同じ分割", canvas: [2000, 1150],
    boxes: [
      ["map", "全国図 (ライブの揺れ)", "map", 0.5, 1.5, 67.5, 97],
      ["status", "状態枠 (状態文 / EEW 札 / 津波パネル)", "overlay", 0.5, 1.5, 31.5, 21],
      ["gauge", "計測震度ゲージ", "overlay", 0.5, 24, 6, 45],
      ["inset", "別枠 (南西諸島)", "overlay", 9, 24, 19, 25],
      ["near", "付近観測点データ", "overlay", 54, 77, 13, 10],
      ["clock", "日時", "overlay", 44.5, 90, 23, 6],
      ["ticker", "下端の帯 (案内)", "bar", 0.5, 95.5, 67.5, 3.5],
      ["sub", "寄り図 + 重ねた詳細", "map", 68.2, 1.5, 31.5, 41.5],
      ["list", "履歴 (大きな行 × 6)", "panel", 68.2, 43.5, 31.5, 55],
    ],
  },
  {
    id: "jdq", label: "参照: JDQ 型 (2×2 のペインの格子)", basis: "概算 (スクリーンショット 2000×1150)", ratio: "上段 ≒ 74% : 下段 ≒ 22%。上段は左 : 右 ≒ 50 : 50。下段の右は情報パネル群 (台風・警報・天気・レーダー・EEW パネル)", canvas: [2000, 1150],
    boxes: [
      ["live", "ペイン: ライブの揺れ (全国図)", "map", 0.6, 1.3, 48.9, 73.9],
      ["status", "状態枠 (EEW 札・津波到達予想)", "overlay", 0.9, 5.5, 21, 14],
      ["inset", "別枠 (南西諸島)", "overlay", 6, 20, 13, 17.5],
      ["recent", "ペイン: 最近の地震 (寄り図)", "map", 50, 1.3, 48.8, 51],
      ["panels", "情報パネル群 (EEW パネル / 履歴 / 台風 / 警報 / 天気 / レーダー)", "panel", 50, 53.5, 48.8, 40],
      ["ticker", "下端の帯 (状態ごとの文)", "bar", 50, 93.5, 48.8, 4.5],
      ["camera", "ライブカメラ (対象外)", "skip", 0.6, 76.5, 21.5, 21.3],
      ["clock", "時計 (アナログ + デジタル)", "overlay", 22.5, 76.5, 27, 21.3],
    ],
  },
];

// 再設計後の部品 (案)。いまの 38 個 (容器 4 を除く。docs の部品カタログ) を、役割で束ねた形
window.LS_COMPONENTS = [
  { id: "map-live", name: "メインの地図", role: "map", merges: "map-interaction / svg.map / 層 / epicenter / offscreen / map-tip / svg-title", slot: "地図の枠", size: "枠いっぱい (fill)", note: "全国図。EEW・津波で自動カメラ。観測点の点群 (将来) を載せる層を持つ", priority: "常に残す" },
  { id: "map-sub", name: "サブの地図 (任意)", role: "map", merges: "(新規。JDQ・JQuake の右上)", slot: "右列の上 (regular の発展形)", size: "右列の約 40%", note: "最新の確定地震を固定ビューで映し、詳細・凡例・出典を重ねる。EEW 追従の有無は選べる", priority: "高 (置けるときだけ)" },
  { id: "status-slot", name: "状態枠 (共有スロット)", role: "overlay", merges: "eew-banner / tsunami-banner / (状態文) / countdown", slot: "地図の左上", size: "幅 約 30〜45%・高さは内容", note: "状態文 → EEW の札 → 津波のパネルを、優先度で 1 つ出す (JQuake・JDQ とも同じ枠を使い回す)", priority: "常に残す" },
  { id: "legend", name: "凡例", role: "overlay", merges: "legend / scale / tsunami / wave / warn", slot: "地図の隅 (隅のスタック)", size: "内容 (縦長)。行は状態で増減", note: "地図ごとに持つ (サブの地図にも)。色の値は設計トークンから", priority: "高" },
  { id: "inset", name: "別枠 (離島)", role: "overlay", merges: "inset-okinawa / inset-ogasawara", slot: "地図の隅のスタック (左上)", size: "高さ = 地図の高さの約 30% (上限 220px)", note: "px 直書きをやめ、地図の高さに対する割合に。案内・カウントダウンはスタックの下に自動で並ぶ", priority: "中 (低い画面では縮める・隠す)" },
  { id: "clock", name: "時計・接続状態", role: "overlay", merges: "clock / c-status", slot: "地図の右下", size: "固定 (181×91 → 縮小版 150×75)", note: "アナログ版 (JDQ) を選べる余地", priority: "高" },
  { id: "quake-detail", name: "地震の詳細 (見出し)", role: "panel", merges: "detail / 詳細の震度札・震源・M・深さ・津波の文言", slot: "右パネルの上、または地図の上の重ね物 (サブの地図)", size: "内容。2 つの形: 文字パネル / 大きな札", note: "領域に依存しない部品。重ねる形 (JDQ・JQuake) と、パネルの形 (今) を切り替える", priority: "常に残す" },
  { id: "quake-list", name: "地震の履歴", role: "panel", merges: "list / list-head", slot: "右パネルの中 (残りの高さ)", size: "残りの高さ (fill)。行は 2 種類: 細い行 (100 件) / 大きな行 (6 件)", note: "件数と行の大きさが違うだけで同じ部品", priority: "高" },
  { id: "warning-view", name: "気象警報の表示", role: "panel", merges: "warn-banner / legend-warn / warn 層 / (JDQ の警報パネル)", slot: "帯 または パネル", size: "帯: 1〜2 行 / パネル: 内容", note: "2 つの形 (帯・パネル)。地図の塗りは地図の層", priority: "中" },
  { id: "tsunami-view", name: "津波の表示", role: "overlay", merges: "tsunami-banner / tsunami 層 / (到達予想の表)", slot: "状態枠 (札) + 地図の沿岸線", size: "情報量の段階: 簡易 (札) / 詳細 (表)", note: "JQuake 型 (札 + 色) と JDQ 型 (表) を、同じ部品の段階として持つ", priority: "高 (出ている間)" },
  { id: "ticker", name: "帯 (テロップ・状態ごとの文)", role: "bar", merges: "telop / bgm-now / (下端の帯)", slot: "上部バー内 または 地図・パネルの下端", size: "1〜2 行", note: "平時は案内、地震では区域・観測点の列挙 (JDQ・JQuake) など、状態ごとの内容を入れる", priority: "中" },
  { id: "toast", name: "通知 (トースト・音の案内)", role: "overlay", merges: "tour-toast / sound-hint", slot: "地図の下端の中央", size: "内容", note: "短い通知。巡回・揺れの報告・音の有効化の案内", priority: "低" },
  { id: "notice", name: "お知らせ", role: "panel", merges: "banner", slot: "右パネルの下", size: "残りの約 1/3 (低い画面では隠す)", note: "現状どおり", priority: "低" },
  { id: "weather", name: "天気 (札・カード)", role: "overlay", merges: "weather-caption / cities 層 / rain 層", slot: "地図の層 (札) または パネル (カード)", size: "札: 地図に追従 / カード: 固定", note: "2 つの形", priority: "低" },
  { id: "controls", name: "操作系 (設定・デモ・音・BGM・全体図・戻る)", role: "control", merges: "settings-open / demo-open / sound / bgm / overview / back-live / calm-now / settings-panel / demo-panel", slot: "上部バー (regular) / 地図の隅のアイコン (JQuake 型) / なし (broadcast)", size: "固定 (小)", note: "broadcast プロファイルでは「出さない」を宣言 (配信用 CSS の ID 列挙が不要になる)", priority: "profile による" },
  { id: "chrome", name: "題名・モード・版・配信元", role: "bar", merges: "title / mode / wave-info / credit", slot: "上部バー", size: "固定", note: "モードの札 + 版 + (配信元ラベル・試験配信の札)", priority: "高" },
];

// 設計トークン (案)
window.LS_TOKENS = [
  ["ブレークポイント (幅)", "compact < 640 / regular ≥ 900 (その間はコンテナの幅で決める)", "いまは 800px の 1 本。横向きスマホで崩れた (844×390)。幅だけでなく、高さ・縦横比も条件に使う"],
  ["ブレークポイント (高さ)", "低い画面 < 480 (横向きスマホ) は、別枠・お知らせ・出典を縮める/隠す優先順位を持つ", "いまは max-height:520 / 440 が min-width:801 と組みで散在"],
  ["領域の比率", "regular: 右パネル = clamp(340px, 29%, 420px) (1280 で 371px、1440 で 418px) / jquake 型: 68 : 32 / jdq 型: 50 : 50 / broadcast-native: 1280×720 の固定 (900 : 380)", "いまは右 380px 固定 (1440 で 26%)。広い画面で地図の割合が自動で増える。割合に変えると、1440 では右が太くなる点に注意"],
  ["縦の予算 (右パネル)", "優先度つきで配分: 詳細 > 履歴 (最低 3 行) > お知らせ > 出典。足りなければ低い順に畳む", "いまは出た順に押し出され、履歴が 0px になる"],
  ["間隔", "4 の倍数 (4/8/12/16/24)。地図の重ね物の隅の余白 = 10px (broadcast は 16px)", "いまは 10・12・14・16 が混在"],
  ["文字の大きさ", "regular 14px 基準 / broadcast は 1.6〜2 倍の大きな札 (震度札・震源名) を持つ", "JDQ・JQuake の特徴は大きな文字。いまの native は 10〜20px が主で、大きな文字は 34px (コードの size 引数で確認した範囲)"],
  ["色", "震度 9 色・津波 3 等級・警報 4 段階・EEW 警報/予報・状態 (接続) をトークン 1 か所から。Rust へは生成して共有", "いまは index.html (凡例)・scale.ts・Rust の 3 か所に重複。#rrggbb の直書きは CSS と TS で 106 か所 (Rust は [u8;3] の定数)"],
  ["重なり順 (z)", "地図 0 / 地図の層 1 / 重ね物 2 / 状態枠 3 / ツールチップ 4 / 帯 5 / モーダル 6", "いまは 1〜3 が部品ごとにばらばら"],
  ["動き", "各部品に「動きあり」の印。reduced-motion と配信 (ヘッドレス) で止めるものを一覧にできる", "いまは CSS の @media に散在 (4 か所)"],
  ["グリッド・スロット", "スロット名 (status / map-main / map-sub / side-top / side-list / side-bottom / ticker …) と、各スロットの行・列・span・比率をプロファイルが宣言。部品は「どのスロットか」だけを持つ", "いまは CSS grid (地図 1fr + 右 380px) と absolute の混在。スロットの定義が無い"],
  ["コンテナクエリ", "container-type を持つ要素: 本体 (.layout) と 地図の枠 (#map)。幅・高さ・縦横比でプロファイルを選ぶ", "いまは viewport の @media のみ。部品が自分の枠の大きさで切り替わらない"],
  ["密度 (文字の段階)", "regular: 12 / 14 / 18 / 24px。broadcast: 札 30 / 36 / 43px など大きな段階を別に持つ", "native は 10〜43px が散在。段階値の一覧が無い"],
  ["safe-area・svh", "env(safe-area-inset-*) を compact の上下の余白に反映。svh を地図の高さに使う (現状は 60svh)", "safe-area は未使用 (時計は bottom:6px)"],
  ["重なり順の現行値", "移行前の対応表: ツールチップ 3 / 案内・トースト・カウントダウン 2 / 重ね物 1 / モバイル上部バー 2。これを上の 7 段に写す", "部品ごとに直書き"],
  ["情報量の段階", "simple (札だけ) / detail (表・一覧) を部品の属性に。JQuake 型 = simple、JDQ 型 = detail", "津波・気象警報・EEW で必要"],
];

window.LS_QUESTIONS = [
  "基準にするのは、どのプロファイルか: web の通常版 (regular) を中心に compact を派生させるのか、配信 (JQuake 型 / JDQ 型) を基準にして web を派生させるのか。",
  "配信用の並びは、JQuake 型 (うちの現行にほぼ近い 2 カラム) と JDQ 型 (2×2 のペイン) のどちらを目指すか。両方を選べるようにするか。",
  "サブの地図 (右上に寄り図) を入れるか。入れるなら、EEW に追従させるか、確定地震の固定ビューか。",
  "状態枠 (状態文 / EEW の札 / 津波のパネル) を共有スロットにしてよいか。複数が同時のときの優先度 (津波 > EEW > 状態文 でよいか)。",
  "情報量の段階 (simple / detail) を、津波・気象警報・EEW のどれに持たせるか。",
  "比率は固定 (割合) か、最小・最大つき (clamp) か。広い画面 (1920 幅) で右パネルを太らせるか。",
  "モバイルで操作系・設定・デモをどう扱うか (シートにする / 地図の隅のアイコンにする / 上部バーのまま)。",
  "色・文字・間隔のトークンを、web と native (Rust) で共有する仕組みを作るか (生成か、手で二重管理を続けるか)。",
];
