// 画面部品の仕様データ (分析結果)。index.html の各表・図は app.js がここから組み立てる。
// 基準: eq-webservice v0.31.0 (f208e1e, 2026-10-05)。確度 conf: "確" = コードを読み、実測でも確認 / "中" = コードからの読み取りで、実機での全状態確認はしていない / "要" = 推測を含む・未確認
// 参照するファイルは web/src/ と web/public/ の相対パス。行番号は変わりやすいので書かず、関数名・定数名で示す。

window.REGIONS = {
  topbar: { name: "上部バー", color: "#4f8cff" },
  banners: { name: "帯 (上部バーの下)", color: "#ff6b6b" },
  map: { name: "地図の枠", color: "#3ecf8e" },
  overlay: { name: "地図の上の重ね物", color: "#f5a524" },
  side: { name: "右パネル", color: "#b983ff" },
};

// ---------------------------------------------------------------------------
// 部品カタログ
// sel: 代表のセレクタ (実測 MEASURED のキーと同じ)
// impl: 描画・更新を担う場所 / data: 元になるデータ / show: 表示条件 / place: 配置の決め方
// couple: 他の部品との暗黙の依存 / native: 配信 (Rust 描画) での対応 / diff: 部品化の難しさ (低・中・高) / note: 補足
// ---------------------------------------------------------------------------
window.COMPONENTS = [
  // ---- 容器 ----
  {
    id: "topbar", name: "上部バー (容器)", region: "topbar", sel: "header.topbar", kind: "container",
    impl: "index.html の header.topbar / style.css .topbar", data: "—",
    show: "常時", place: "body (flex 縦) の先頭。flex 横並び・gap 10px。幅 800px 以下で折り返し (flex-wrap) + position:sticky",
    couple: "高さが可変 (実測 PC 37px / スマホ 91px)。下の帯・地図の開始位置がこの高さで決まる (どこにも px で書かれていない)",
    native: "draw_frame: 36px (BAR_H) の固定帯", diff: "低", conf: "確",
  },
  {
    id: "layout", name: "本体 (地図 + 右パネル)", region: "map", sel: "main.layout", kind: "container",
    impl: "style.css .layout", data: "—",
    show: "常時", place: "PC: grid 1fr 380px (flex:1 で残りの高さ全部) / スマホ: display:block で縦積み (ページ全体がスクロール)",
    couple: "帯 (EEW・津波・警報) が出ると、その分だけ地図と右パネルの高さが縮む (重ならずに押し下げる)",
    native: "地図 (x 0..900) と右パネル (x 900..1280) の固定分割。帯は上に重ねる", diff: "中", conf: "確",
  },
  {
    id: "map", name: "地図の枠", region: "map", sel: "#map", kind: "container",
    impl: "index.html section#map.map-wrap / style.css .map-wrap / map.ts JapanMap のコンストラクタ", data: "—",
    show: "常時", place: "position:relative; overflow:hidden。PC は grid の左列、スマホは高さ 60svh",
    couple: "重ね物 (時計・凡例・別枠・案内…) はすべてこの枠に position:absolute で置かれる。さらに JapanMap が別枠・ツールチップ・画面外矢印の要素をこの枠へ自分で append する",
    native: "MAP_RECT = (0, 36, 900, 684) の固定", diff: "高", conf: "確",
    note: "地図の描画面 (svg.map) と、地図の上の重ね物が同じ容器に同居している。部品を並べ替えるなら最初に分ける必要がある",
  },
  {
    id: "side", name: "右パネル (容器)", region: "side", sel: "aside.side", kind: "container",
    impl: "style.css .side", data: "—",
    show: "常時", place: "PC: 幅 380px の flex 縦 (min-height:0)。中の履歴 (#list) が flex:1 で余りを取り、お知らせ (#banner) と半分ずつ分ける。スマホ: 地図の下に縦積み",
    couple: "縦の予算が足りないと履歴が 0px まで潰れる (実測: PC 1440×900 でデモパネル + EEW 詳細を出すと #list の高さ 0)",
    native: "x 900..1280。詳細 (上)・履歴 (中)・お知らせ・出典 (下) を固定の y で描く", diff: "中", conf: "確",
  },

  // ---- 上部バー ----
  {
    id: "title", name: "題名とバージョン", region: "topbar", sel: ".topbar h1", kind: "static",
    impl: "index.html (%VERSION% を web/build.mjs が Cargo.toml の version に置換)", data: "Cargo.toml workspace.package.version",
    show: "常時 (スマホは版を隠す .version{display:none})", place: "flex 項目 (flex:none, nowrap)",
    couple: "なし", native: "draw_frame が同じ文言を描く (x=16, y=24)", diff: "低", conf: "確",
  },
  {
    id: "mode", name: "表示モードの札", region: "topbar", sel: "#mode", kind: "status",
    impl: "view.ts renderMode()", data: "app.demo / app.selectedKey / app.tourKey",
    show: "常時。data-mode が live(緑) / replay(青) / demo(橙) / tour(青緑) で文言と色が変わる", place: "flex 項目",
    couple: "なし。ただし #back-live の表示と同じ関数 (renderMode) が決める", native: "意味が違う: native の「[平時]/[地震]」は画面の状態 (x=210)。web の「リアルタイム/リプレイ中」に相当するものは無い",
    diff: "低", conf: "確",
  },
  {
    id: "back-live", name: "「リアルタイムに戻る」ボタン", region: "topbar", sel: "#back-live", kind: "action",
    impl: "view.ts renderMode() / main.ts のクリック処理", data: "app.demo, app.selectedKey, map.userMoved",
    show: "モードが live/tour 以外、または利用者が地図を動かした (map.userMoved) とき。デモ中に地震を選ぶと文言が「自動表示に戻る」", place: "flex 項目",
    couple: "なし", native: "なし (操作なし)", diff: "低", conf: "確", note: "配信 (?broadcast=1) では CSS が隠す",
  },
  {
    id: "overview", name: "「全体図」ボタン", region: "topbar", sel: "#overview", kind: "action",
    impl: "index.html / main.ts のクリック処理 → map.showOverview()", data: "—",
    show: "常時 (配信では CSS で隠す)", place: "flex 項目", couple: "なし", native: "なし", diff: "低", conf: "確",
  },
  {
    id: "calm-now", name: "「警報・注意報」ボタン", region: "topbar", sel: "#calm-now", kind: "action",
    impl: "main.ts renderWarnings()", data: "calmState() の結果, app.demo",
    show: "地震の表示中 (calm でない) かつ津波予報なし・デモでない。押すと平時表示へ戻す (app.calmSince)", place: "flex 項目",
    couple: "なし", native: "なし", diff: "低", conf: "確",
  },
  {
    id: "wave-info", name: "波の経過時間", region: "topbar", sel: "#wave-info", kind: "status",
    impl: "scene.ts renderScene() が textContent を書き、main.ts tick() が空にする", data: "scene() の t (発生からの秒数)",
    show: "P波・S波を描いている間 (「発生から19秒」「再生中 24秒 (×3)」)", place: "flex 項目 (色は --s-wave)",
    couple: "表示の有無でバー内の幅が変わり、テロップの幅 (flex:1) が増減する", native: "なし", diff: "低", conf: "確",
    note: "配信用 CSS では隠されない (操作ボタンではないため)",
  },
  {
    id: "telop", name: "テロップ", region: "topbar", sel: "#telop", kind: "ticker",
    impl: "chrome.ts renderTelop() / loadTelop()", data: "GET /api/telop (起動時 1 回)。8 秒ごとに次の文へ (TELOP_INTERVAL_MS)",
    show: "平常時のみ。波・EEW・優先する地震・津波予報・デモのいずれかがあると hidden (busy)", place: "flex:1 1 auto; min-width:0; 1 行で省略 (ellipsis)。スマホは order:10 + flex-basis:100% で別の行へ",
    couple: "PC では残りの幅を全部取る → 他のボタンが増えると短くなる。スマホは 1 行ぶん高さを足す (topbar 91px の一因)", native: "なし (native の telops.rs は気象庁の天気アイコンの表で、テロップではない)", diff: "低", conf: "確",
  },
  {
    id: "settings-open", name: "「設定」ボタン", region: "topbar", sel: "#settings-open", kind: "action",
    impl: "personal-ui.ts (#settings-panel を開閉)", data: "—", show: "常時 (配信では隠す)", place: "flex 項目", couple: "なし", native: "なし", diff: "低", conf: "確",
  },
  {
    id: "demo-open", name: "「デモ」ボタン", region: "topbar", sel: "#demo-open", kind: "action",
    impl: "demo-ui.ts enterDemo() / view.ts renderMode()", data: "—", show: "デモ・履歴再生中は隠す (配信でも隠す)", place: "flex 項目", couple: "なし", native: "なし", diff: "低", conf: "確",
  },
  {
    id: "sound", name: "「音」ボタン", region: "topbar", sel: "#sound", kind: "action",
    impl: "chrome.ts renderSound()", data: "sound.ts の soundEnabled / soundReady",
    show: "常時 (配信では隠す)。文言は OFF / ON / ON(タップで有効化) の 3 状態", place: "#sound{margin-left:auto} で、ここから右の部品を右端へ寄せる",
    couple: "margin-left:auto の位置で上部バーの左右グループが分かれる。この ID が並びの境目を兼ねている", native: "なし", diff: "低", conf: "確",
  },
  {
    id: "bgm", name: "「BGM」ボタン", region: "topbar", sel: "#bgm", kind: "action",
    impl: "bgm.ts renderBgm()", data: "GET /api/bgm (起動時 1 回) が無ければ hidden",
    show: "サーバに BGM の設定があるときだけ", place: "flex 項目", couple: "なし", native: "なし (BGM は mixer 側)", diff: "低", conf: "確",
  },
  {
    id: "bgm-now", name: "BGM の曲名", region: "topbar", sel: "#bgm-now", kind: "ticker",
    impl: "bgm.ts renderBgm()", data: "Icecast の status-json.xsl の title",
    show: "再生中のみ", place: "max-width:16em。収まらないと左右に往復して流す (pingpong アニメーション。--bgm-shift を JS が計算)",
    couple: "なし", native: "draw_top_right が「BGM: 曲名」を上部バー右端付近に描く (15 秒ごと取得)", diff: "低", conf: "確",
  },

  // ---- 帯 ----
  {
    id: "eew-banner", name: "緊急地震速報の帯", region: "banners", sel: "#eew-banner", kind: "banner",
    impl: "view.ts renderBanner()", data: "activeEews(now)。最大 3 件 (EEW_BANNER_MAX) + 「ほか N 件」",
    show: "発表中の緊急地震速報があるとき (role=alert)。警報は赤で点滅、予報だけなら橙", place: "上部バーの下に通常のフローで置く → 出ると地図と右パネルの高さが縮む (実測: 1 件 39px / 2 件 61px / スマホ 3 行 100px)",
    couple: "高さが可変なので、地図の枠の高さと開始位置が変わる (地図の上端に固定した重ね物は一緒に下がる)", native: "対応する帯は無い (右パネルの EEW 詳細 + 地図の予測の塗り・波の円で表す)", diff: "中", conf: "中",
  },
  {
    id: "tsunami-banner", name: "津波予報の帯", region: "banners", sel: "#tsunami-banner", kind: "banner",
    impl: "view.ts renderTsunamiBanner()", data: "activeAreas(app.world.tsunami)",
    show: "津波予報が発表中 (解除まで)。data-grade で色 (注意報=黄 / 警報=赤 / 大津波警報=紫)", place: "通常フロー (EEW の帯の下)", couple: "同上 (高さが可変)", native: "なし (津波の地図の塗りも帯も無い。詳細に「津波」の文言のみ)", diff: "低", conf: "中",
  },
  {
    id: "warn-banner", name: "気象警報の帯", region: "banners", sel: "#warn-banner", kind: "banner",
    impl: "main.ts renderWarnBanner() / warnings.ts warningSummary()", data: "GET /api/warnings (5 分ごと)",
    show: "平時 (calm) で警報以上があるとき。注意報だけなら出さない", place: "通常フロー。長いと横に流す (.scroll、速さ --warn-sec)。幅の変化でも再判定 (clientWidth を署名に含める)",
    couple: "幅 (clientWidth) に依存して流すかを決めるので、部品の幅が変わったら再計算が必要", native: "banner.rs: 同じ規則・文・色。ただし流さず 2 行まで + 「ほか N 件」。上に重ねる (押し下げない)", diff: "中", conf: "確",
  },

  // ---- 地図の上の重ね物 ----
  {
    id: "countdown", name: "主要動の到達カウントダウン", region: "overlay", sel: "#countdown", kind: "overlay",
    impl: "personal-ui.ts renderCountdown()", data: "自分の地点 (設定) + 発表中の EEW の震源・発生時刻",
    show: "地点を設定済みで、揺れそうな EEW があるときだけ (自分の区域が対象か、推定震度 3 以上)",
    place: "position:absolute; left:12px; top:12px。南西諸島の別枠が出ているときは top:242px (CSS の :has で切替)。スマホは bottom:64px の中央",
    couple: "別枠 (高さ 220px) の高さ + 余白 (= 242) を直書き。別枠の大きさを変えるとここも直す必要がある", native: "なし", diff: "高", conf: "確",
  },
  {
    id: "tour-toast", name: "巡回・揺れの報告のトースト", region: "overlay", sel: "#tour-toast", kind: "overlay",
    impl: "main.ts showToast()", data: "巡回で移った地震 / 地震感知情報の新しい揺れ",
    show: "2.5 秒だけ (アニメーションで出て消える)", place: "left:50%; bottom:60px (中央・下から 60px)", couple: "#sound-hint (bottom:14px) と下端付近を共有", native: "なし", diff: "低", conf: "確",
  },
  {
    id: "weather-caption", name: "天気の札の案内", region: "overlay", sel: "#weather-caption", kind: "overlay",
    impl: "weather-layer.ts renderCityWeather()", data: "weatherView(): 「現在の天気」/「明日 10/6 (火) の天気」",
    show: "平時で、設定「天気の札の切り替え」が「出さない」でないとき (地震の表示中は消す)", place: "left:246px; top:10px。スマホは left:158px",
    couple: "246 = 南西諸島の別枠の left(10) + 幅(226) + 余白(10) の手計算。スマホの 158 も別枠 (高さ 140 → 幅 144) からの手計算。別枠の大きさ・位置を変えると狂う",
    native: "INFO_WINDOW = (274, 198, 242, 44) (calm.rs)。web と同じく別枠の右側だが、y が違う (web は上端 10px、native は y=198)。文言も別に管理", diff: "高", conf: "確",
  },
  {
    id: "sound-hint", name: "音の有効化の案内", region: "overlay", sel: "#sound-hint", kind: "overlay",
    impl: "chrome.ts renderSound()", data: "音の設定は ON だがブラウザの制限でまだ鳴らせない状態",
    show: "上記のときだけ (配信では隠す)", place: "left:50%; bottom:14px", couple: "時計・凡例と下端付近を共有 (中央なので通常は重ならない)", native: "なし", diff: "低", conf: "確",
  },
  {
    id: "clock", name: "日時・接続状態の時計", region: "overlay", sel: "#clock", kind: "overlay",
    impl: "chrome.ts renderClock() / setStatus()", data: "サーバ時刻 (Connection.now) と WebSocket の状態",
    show: "常時。枠の色が接続状態を示す (緑=同期 / 赤=切断中 / 黄=接続中)", place: "position:absolute; right:10px; bottom:10px。pointer-events:none。実測 PC 181×91 / スマホ 150×75",
    couple: "小笠原の別枠が「時計の上」(bottom:96px) に置かれる (時計の高さ 91 + 余白に依存)。thinCities の「札を置けない場所」としても DOM から読まれる",
    native: "draw_clock: 176×74 を右下 (x=714,y=636) に固定で描く", diff: "中", conf: "確",
  },
  {
    id: "legend", name: "凡例 (容器)", region: "overlay", sel: ".legend", kind: "overlay",
    impl: "index.html .legend (静的) + 各行の hidden は main.ts / view.ts が切替", data: "—",
    show: "常時 (中の行が状況で増減)", place: "left:10px; bottom:10px (縦に長い。スマホは bottom:auto; top:158px へ移動)。高さは行の数で変わる (実測 PC 147〜238px)",
    couple: "高さが可変なので、南西諸島の別枠 (上端 10px・高さ 220px) と重ならない保証が無い: 高さの低い画面 (@media max-height:520px) で縮める規則を別に持つ。実測 844×390 では別枠と約 100px 重なり、震度ゲージが隠れる",
    native: "LEGEND_RECT = (10, 563, 34, 147) + 警報の凡例 (66×78) をその上に固定で描く。津波・P波S波の行は無い", diff: "高", conf: "確",
  },
  {
    id: "legend-scale", name: "凡例: 震度のゲージ", region: "overlay", sel: ".legend .scale", kind: "overlay", parent: "legend",
    impl: "index.html (9 色の <i> を直書き)", data: "scale.ts の COLORS と同じ値を HTML に重複して書いている", show: "常時", place: "凡例の最上段 (縦ゲージ、上が強い)", couple: "色の値が scale.ts / native の scale_color と 3 か所に重複", native: "GAUGE (panel.rs) に同じ 9 色", diff: "低", conf: "確",
  },
  {
    id: "legend-tsunami", name: "凡例: 津波の等級", region: "overlay", sel: "#legend-tsunami", kind: "overlay", parent: "legend",
    impl: "main.ts renderAll()", data: "activeAreas(tsunami)", show: "津波予報が発表中のとき", place: "凡例内の行 (flex 縦)", couple: "出ると凡例の高さが増える", native: "なし", diff: "低", conf: "確",
  },
  {
    id: "legend-wave", name: "凡例: P波・S波", region: "overlay", sel: "#legend-wave", kind: "overlay", parent: "legend",
    impl: "main.ts tick()", data: "waving", show: "波を描いている間", place: "凡例内の行", couple: "同上", native: "なし (波の円は描くが凡例は無い)", diff: "低", conf: "中",
  },
  {
    id: "legend-warn", name: "凡例: 気象警報の段階", region: "overlay", sel: "#legend-warn", kind: "overlay", parent: "legend",
    impl: "main.ts renderWarnings()", data: "塗る区域が 1 つ以上あるとき", show: "平時で警報・注意報の塗りがあるとき", place: "凡例内の行", couple: "同上 (実測 85px の増加)", native: "draw_warn_legend (平時のみ、固定位置)", diff: "低", conf: "確",
  },
  {
    id: "inset-okinawa", name: "別枠: 南西諸島", region: "overlay", sel: ".inset-okinawa", kind: "overlay",
    impl: "map.ts INSETS / updateInsets() / renderInsetMarkers()。要素は JapanMap が生成して #map に append", data: "本図の層 (<use href=\"#map-base\">) を縮小して映す + 震央・天気札を別に描く",
    show: "日本全体 (全体図) を見ているときだけ。条件は `!zoomed && view.h >= homeView().h * 0.8` なので、手動で少し寄っただけでも隠れる。always:true なので範囲に何も無くても出る", place: "left:10px; top:10px; 高さ 220px (幅は縦横比から 226px)。スマホは高さ 140px",
    couple: "weather-caption の left / countdown の top / スマホの凡例の top の 3 か所がこの寸法の手計算。thinCities の「置けない場所」でもある。CSS を <use> の複製に効かせるため、図形のスタイルは SVG の中 (map.css) に埋め込んでいる",
    native: "OKINAWA (frame.rs): x=10, y=46, h=220 で同じ範囲 (東経 122.5–131.5, 北緯 24–31)", diff: "高", conf: "確",
  },
  {
    id: "inset-ogasawara", name: "別枠: 小笠原", region: "overlay", sel: ".inset-ogasawara", kind: "overlay",
    impl: "map.ts INSETS (always:false)", data: "同上", show: "全体図で、小笠原の範囲に震央・塗り・津波などがあるときだけ", place: "right:10px; bottom:96px; 高さ 150px (幅は約 52px の細長い枠)。スマホは bottom:70px・高さ 100px",
    couple: "bottom:96 は時計 (高さ約 91) の上に載せる手計算", native: "未実装 (docs/nansei-inset.md: 「今回は作らない (別の宿題)」と先送り)", diff: "高", conf: "中", note: "平時は出ないので実測できていない。値は CSS の読み取り",
  },
  {
    id: "map-tip", name: "地図のツールチップ", region: "overlay", sel: ".map-tip", kind: "overlay",
    impl: "map.ts installTooltip() / tipText()", data: "指した地域の名前と震度 (スマホではタップ)", show: "ポインタが地域上にあるとき", place: "ポインタ位置 + 12px に absolute", couple: "なし (#map の子。z-index:3)", native: "なし", diff: "低", conf: "確",
  },
  {
    id: "offscreen", name: "画面外の地震の矢印", region: "overlay", sel: ".offscreen-layer", kind: "overlay",
    impl: "map.ts renderOffscreen() (innerHTML)", data: "番号付きの震央のうち、いまの表示範囲の外にあるもの",
    show: "寄っていて、ほかの地震が画面外のとき (押すとその地震へ)", place: "地図の端 (edgePoint) に absolute。層全体は inset:0 で pointer-events:none",
    couple: "地図の表示範囲 (view) と枠の実寸 (getBoundingClientRect) に依存", native: "なし", diff: "中", conf: "確",
  },

  // ---- 地図の操作・印 (レビューで追加) ----
  {
    id: "map-interaction", name: "地図の操作 (パン・ズーム・地点選択)", region: "map", sel: "svg.map", kind: "behavior",
    impl: "map.ts installPanZoom() / pickPoint() / showOverview()", data: "pointer / wheel / ピンチ。利用者が動かすと自動カメラを止める (map.userMoved)",
    show: "常時。「自分の地点を地図で選ぶ」の間は .picking (十字カーソル)", place: "svg に touch-action:none; cursor:grab",
    couple: "動かしたことが #back-live の表示に影響する (renderMode)", native: "なし (操作なし)", diff: "中", conf: "確",
    note: "モバイルではページのスクロールと地図のドラッグが競合しうる (touch-action:none)。プロファイルごとに操作の有無・方法を変える単位",
  },
  {
    id: "epicenter", name: "震央の印 (✕ と番号)", region: "overlay", sel: ".epicenter", kind: "overlay",
    impl: "map.ts setEpicenters() / renderMarkers()。scene.ts renderMarkers() が内容を決める", data: "直近の地震と選択中の地震。履歴再生では「のちに判明する震源」を薄い点線で出す (ghost)",
    show: "直近の地震があるとき", place: "SVG の markers 層。.epicenter-hit (半径 16) で押せる (押すとその地震を選ぶ)。別枠の中の印は pointer-events:none で押せない",
    couple: "画面外の矢印 (offscreen) と同じ onSelect を使う", native: "✕ と番号は描く。ghost は draw_hindsight", diff: "中", conf: "中",
  },
  {
    id: "svg-title", name: "SVG の <title> による標準ツールチップ", region: "overlay", sel: "svg.map title", kind: "behavior",
    impl: "map.ts (都道府県・細分区域・津波線・揺れの報告)、weather-layer.ts (天気の札)", data: "名前・震度・天気",
    show: "ブラウザの標準のホバー表示", place: "ブラウザ任せ", couple: ".map-tip (独自のツールチップ) とは別系統で並存", native: "なし", diff: "低", conf: "中",
  },

  // ---- 右パネル ----
  {
    id: "settings-panel", name: "設定パネル", region: "side", sel: "#settings-panel", kind: "panel",
    impl: "personal-ui.ts (renderSettings / イベント)", data: "app.settings → localStorage \"eq-settings\"",
    show: "「設定」を押したときだけ (切替)", place: "右パネルの最上段 (通常フロー)",
    couple: "開くと詳細・履歴の縦の予算が減る。スマホでは地図の下に出るので、押しても見えない位置になりうる", native: "なし", diff: "中", conf: "確",
    note: "中身: 自分の地点 (選ぶ/現在地/解除)、通知レベル、観測点一覧の畳み方、履歴の下限震度、巡回の間隔、天気の札の切替、BGM 音量、読み上げ",
  },
  {
    id: "demo-panel", name: "デモ・履歴再生パネル", region: "side", sel: "#demo-panel", kind: "panel",
    impl: "demo-ui.ts renderDemoPanel() / renderDemoControls()", data: "demo/index.json (場面の一覧) と demo/<id>.json、履歴再生は /api/archive",
    show: "デモモード中・履歴再生中", place: "右パネルの上段 (設定の下)。リストは max-height:40vh でスクロール",
    couple: "実測 481px 高 (PC)。出ている間は詳細・履歴が圧迫される (履歴 0px)", native: "なし", diff: "中", conf: "確",
    note: "中身: 見出し (デモ/履歴で切替)、再生操作 (一時停止・×1/2/4/8・シーク・時間・早送り)、場面の一覧",
  },
  {
    id: "detail", name: "詳細パネル", region: "side", sel: "#detail", kind: "panel",
    impl: "view.ts renderDetail() (innerHTML)。内容は 6 通り: quake / eew / tsunami、初期の「接続しています…」、「受信した情報はまだありません」、読み込み失敗 (.error)", data: "currentGroup() (選択中 or 優先順位の最上位の地震グループ)",
    show: "常時 (受信なしは「受信した情報はまだありません」)。aria-live=polite", place: "PC: max-height:55% で overflow:auto (縦が低い画面は 40%) / スマホ: max-height なし",
    couple: "中の観測点一覧 (<details class=points-box>) は設定の分数で自動で畳む (updatePointsOpen)", native: "draw_detail / draw_eew_detail: バッジ y=50〜58、区切り線 y=180 に固定", diff: "中", conf: "確",
  },
  {
    id: "list", name: "履歴の一覧", region: "side", sel: "#list", kind: "panel",
    impl: "view.ts renderList() (innerHTML)。行は quake / eew / tsunami の 3 種 (eew_detection のテンプレートはあるが一覧から除外)。選択中 .selected、ホバーあり", data: "app.world.store.list() (最大 100 件)。設定「履歴に出す地震」で絞る",
    show: "常時 (見出し「履歴」と一緒)", place: "PC: flex:1 + overflow:auto で余りを取る / スマホ: overflow:visible (ページ全体で流す)。行を押すとその地震を選ぶ (履歴再生)",
    couple: "お知らせ (#banner) と余りを半分ずつ分ける (flex:1 1 0)。詳細・デモ・設定が増えると 0px まで縮む", native: "draw_history: 最大 5 件の固定表示 (スクロールなし)", diff: "中", conf: "確",
  },
  {
    id: "banner", name: "お知らせ (平時のバナー)", region: "side", sel: "#banner", kind: "panel",
    impl: "banner.ts updateBanner() / loadBanners()", data: "GET /api/banners (interval_sec ごとに次へ。一巡で取り直し)。画像・文字・リンク。HTML は挿入しない",
    show: "平時 (quiet) で項目があるとき。デモ・地震の表示中は隠す", place: "PC: flex:1 1 0 (履歴と半分ずつ)。スマホ: flex:none; max-height:50vh。横向きスマホ (高さ 440px 以下) は display:none",
    couple: "履歴の高さと競合 (同じ flex 余白を分け合う)", native: "notice.rs: 文字のみ・最大 4 行の箱を y=492 に固定。画像・リンクは出さない", diff: "中", conf: "確",
  },
  {
    id: "credit", name: "出典・クレジット", region: "side", sel: "footer.credit", kind: "static",
    impl: "index.html footer.credit (+ #credit-wolfx は Wolfx 経由の情報を受けたら表示)", data: "静的な文言",
    show: "常時。高さ 520px 以下の PC 幅は 1 行に切って隠す (全文はテロップで流す)", place: "右パネルの最下段 (実測 PC 100px 高)",
    couple: "出典の文言が web / native / README の 3 か所に別々に書かれている", native: "CREDIT (panel.rs) の 6 行を右パネルの下に描く", diff: "低", conf: "確",
  },
];

// ---------------------------------------------------------------------------
// 地図 SVG の層 (描画順。下が先 = 奥)
// base: <use href="#map-base"> で南西諸島・小笠原の別枠に複製される層
// ---------------------------------------------------------------------------
window.LAYERS = [
  { order: 1, name: "neighbors", what: "周辺国の陸地 (背景)", data: "neighbors.geojson", when: "常時", base: true, impl: "map.ts loadNeighbors()", note: "観測範囲外の背景。薄い色" },
  { order: 2, name: "prefs", what: "都道府県 (陸の基本の塗り)", data: "japan.geojson", when: "常時。全体図では震度の色で塗る。寄ると (.zoomed) 塗りをやめて細分区域に任せる", base: true, impl: "map.ts setPrefScales() / setFade()", note: "予測 (EEW) の間は点滅 (forecast-pulse)。震度表示は fadeOpacity で薄れる" },
  { order: 3, name: "warn", what: "気象警報・注意報の塗り (市町村等)", data: "warning-areas.geojson (初めて必要になった時に読む) + /api/warnings", when: "平時 (calm) のみ", base: true, impl: "map.ts setWarnings()", note: "段階 4 色。天気の札はこの上に重なるので、警報以上の区域を避けて札を動かす" },
  { order: 4, name: "rain", what: "アメダスの雨の点", data: "/api/weather の rain", when: "平時 (calm) のみ", base: true, impl: "weather-layer.ts (dotPaths)", note: "強さで色分けした長さ 0 の線 (丸い線端)" },
  { order: 5, name: "areas", what: "地震情報細分区域 (寄ったときの塗り)", data: "areas.geojson", when: "寄っている (.zoomed) ときだけ", base: true, impl: "map.ts setDetail()", note: ".map:not(.zoomed) .areas は display:none" },
  { order: 6, name: "tsunami", what: "津波予報区の沿岸線 (等級の色)", data: "tsunami.geojson", when: "津波予報の発表中 (解除まで)", base: true, impl: "map.ts setTsunami()", note: "" },
  { order: 7, name: "dots", what: "震度観測点の点", data: "stations.json", when: "寄っているときだけ", base: true, impl: "map.ts setDetail()", note: "" },
  { order: 8, name: "wave", what: "P波・S波の円", data: "waves.ts (一様速度の概算)", when: "発生から 180 秒まで (WAVE_MAX_SEC)。行を選ぶ再生 (selectedKey、×3) は揺れた地域を覆い終えたら打ち切り", base: true, impl: "map.ts setWaves()", note: "別枠にも描く" },
  { order: 9, name: "labels", what: "震度の数字の札 (地域の内側)", data: "labels.ts pickLabels()", when: "塗り分けがあるとき", base: false, impl: "map.ts renderLabels()", note: "別枠には複製しない" },
  { order: 10, name: "cities", what: "主要都市の天気の札", data: "/api/weather", when: "平時 (calm) のみ。設定で札だけ消せる", base: false, impl: "weather-layer.ts / thin.ts", note: "凡例・別枠・時計・案内・警報以上の区域を避けて動かし、置けなければ間引く。那覇の札は別枠用に複製する" },
  { order: 11, name: "markers", what: "震央の ✕ と番号・揺れの報告の輪・自分の地点", data: "scene.ts renderMarkers() / userquake / settings.home", when: "震央: 直近の地震 / 輪: 地震感知情報 / 自分の地点: 設定済みのとき", base: false, impl: "map.ts setEpicenters / setUserquake / setHome", note: "別枠の中の震央は縮尺を合わせて別に描く (insetMarkerPos)" },
];

// ---------------------------------------------------------------------------
// 表示条件 (状態 × 部品)。●=出る ○=条件つき －=出ない
// 状態は main.ts の calmState() と app の状態から読み取った。各セルの根拠は note に書く
// ---------------------------------------------------------------------------
window.STATES = [
  { id: "calm", name: "平時", desc: "デモでなく、揺れの報告・津波予報・発表中の EEW・波が無く、過去の地震を選んでもいない (calm = true)" },
  { id: "eew", name: "EEW 中", desc: "発表中の緊急地震速報がある (波の円も出る)" },
  { id: "quake", name: "地震情報", desc: "EEW は終わったが、地震情報 (震度速報・各地の震度) を優先して見せている間 (落ち着く = settled になるまで)" },
  { id: "tsunami", name: "津波予報", desc: "津波予報が発表中 (解除まで。単独でも、地震と同時でも)" },
  { id: "feel", name: "揺れの報告", desc: "地震感知情報 (利用者の「揺れた」報告) を地図に出している間" },
  { id: "replay", name: "履歴再生", desc: "履歴の行を選んだ再生。地震は demo.history (当時の時刻で ×1〜8、「発生から N秒」)、発生時刻が分からない行・デモ中の選択は selectedKey (波だけ ×3、「再生中 N秒 (×3)」)。2 つの仕組みがあり、この表では同じ列にまとめている" },
  { id: "demo", name: "デモ", desc: "デモモード (架空の場面・過去の記録を再生)" },
];
// コンポーネントの id → 各状態のセル [calm, eew, quake, tsunami, feel, replay, demo]、補足
window.MATRIX = [
  ["mode", "●●●●●●●", "常時。文言は live/replay/demo/tour で変わる"],
  ["telop", "○－－－●－－", "busy (波・EEW・優先する地震・津波・デモ) の間は消す。平時でも、軽い地震のあと「落ち着く」(settle) から 10 分 (priorityGroups の FULL_MS) までは calm なのにテロップが消える (レビュー指摘)"],
  ["eew-banner", "－●○○－○○", "発表中の EEW は受信から 3 分 (EEW_BANNER_MS) 残るので、地震情報が届いても出続けうる。再生・デモでも場面に EEW があれば出る"],
  ["tsunami-banner", "－○○●－○○", "津波予報が発表中ならいつでも出る (EEW や地震と同時もある)"],
  ["warn-banner", "○－－－－－－", "平時で警報以上があるときだけ"],
  ["wave-info", "－●○○－●○", "波を描いている間 (発生から 180 秒まで)"],
  ["calm-now", "－●●－●－－", "calm でなく、津波予報なし・デモでないとき"],
  ["back-live", "－－－－－●●", "ほかに、利用者が地図を動かしたとき"],
  ["demo-open", "●●●●●－－", "デモ・履歴再生中は隠す"],
  ["countdown", "－○○○－○○", "自分の地点を設定済みで、揺れそうな EEW があるとき (activeEews を見るので、履歴再生中も出る)"],
  ["weather-caption", "○－－－－－－", "設定で消せる。平時のみ"],
  ["legend-warn", "○－－－－－－", "警報・注意報の塗りがあるとき"],
  ["legend-wave", "－●○○－●○", "波を描いている間"],
  ["legend-tsunami", "－○○●－○○", "津波予報が発表中のとき"],
  ["inset-okinawa", "●○○○●○○", "全体図のときだけ。地震へ寄っている間は隠す"],
  ["demo-panel", "－－－－－●●", "履歴再生は見出しだけ。デモは場面の一覧つき"],
  ["detail", "●●●●●●●", "常時。中身は quake / eew / tsunami で 3 通り"],
  ["list", "●●●●●●●", "常時"],
  ["banner", "○－－－－－－", "平時 (quiet) で項目があるとき。BGM の再生も同じ条件"],
  ["offscreen", "－○○－－○○", "寄っていて、ほかの番号付き地震が画面外にあるとき"],
];

// ---------------------------------------------------------------------------
// 配置の結合 (px 直書きの暗黙の依存)
// ---------------------------------------------------------------------------
window.COUPLINGS = [
  { id: "C1", title: "南西諸島の別枠 ↔ 天気の札の案内", rule: "weather-caption の left = 246px (スマホ 158px)", derived: "別枠 left 10 + 幅 226 + 余白 10 / スマホは 10 + 144 + 4", where: "style.css .weather-caption, @media (max-width:800px)", risk: "別枠の高さ・位置を変えると案内が別枠に重なるか、離れすぎる。幅は縦横比から JS が決める (226 は実測) ので CSS だけでは分からない", ev: "実測: 別枠 x10..236 / 案内 x246" },
  { id: "C2", title: "南西諸島の別枠 ↔ カウントダウン", rule: "別枠が出ているときだけ countdown の top を 12 → 242px に", derived: "別枠 top 10 + 高さ 220 + 余白 12", where: "style.css `.map-wrap:has(.inset-okinawa:not([hidden])) .countdown`", risk: ":has() に依存 (古いブラウザでは効かない)。別枠の高さ変更に追従しない。スマホは別の規則 (bottom:64px) で逃げている", ev: "CSS" },
  { id: "C3", title: "南西諸島の別枠 ↔ 凡例 (スマホ・低い画面)", rule: "スマホは凡例を top:158px へ移動 / 低い画面は凡例の行高を 15 → 10px に縮める", derived: "別枠 (高さ 140) の下に置くため", where: "style.css @media (max-width:800px) と (max-height:520px)", risk: "高さの低い画面の規則 (max-height:520px) は凡例の行高だけを縮め、別枠 (220px) は縮めない。そのため 844×390 のように地図が低いと、縮めた凡例でも別枠と重なる", ev: "実測 844×390: 別枠 y78..298 / 凡例 y196..380 (x は両方 10 から) → 約 100px 重なり、震度ゲージが別枠の下に隠れる (img/phone-landscape-calm.png)" },
  { id: "C4", title: "時計 ↔ 小笠原の別枠", rule: "小笠原の別枠は bottom:96px (スマホ 70px)", derived: "時計の bottom 10 + 高さ 約 91 − α (スマホは 75)", where: "style.css .inset-ogasawara", risk: "時計の文字の大きさを変えると重なる / 離れる", ev: "実測 時計 181×91 (PC) / 150×75 (スマホ)。小笠原の別枠は平時に出ないため未実測" },
  { id: "C5", title: "札の置き場所の判定 ↔ 部品の DOM セレクタ", rule: "map.ts thinCities() が `.legend, .clock-panel, .weather-caption:not([hidden]), .inset:not([hidden])` を querySelectorAll して、札を置けない四角にする", derived: "—", where: "map.ts thinCities() / thin.ts", risk: "新しい重ね物 (部品) を足しても、このセレクタに書き足さないと札が上に重なる。逆に部品を消すと selector が空振りする。「置けない場所」を部品側が宣言する形になっていない", ev: "コード" },
  { id: "C6", title: "地図の枠 ↔ 子の要素の生成", rule: "JapanMap のコンストラクタが、別枠 (.inset)・ツールチップ (.map-tip)・矢印の層 (.offscreen-layer) を container (#map) へ append する", derived: "—", where: "map.ts constructor", risk: "地図の描画と、地図の上の重ね物が同じクラスに入っている。重ね物を別の場所へ移したり、モバイルで別の親へ置いたりできない", ev: "コード" },
  { id: "C7", title: "帯の高さ ↔ 地図・右パネルの高さ", rule: "EEW・津波・警報の帯は通常フローに置く", derived: "帯の行数で高さが変わる (39〜100px)", where: "index.html / style.css", risk: "帯が出入りするたびに地図の高さが変わり、地図のカメラ (ResizeObserver で applyView) と、札の置き直しが走る。native は帯を重ねるので挙動が違う", ev: "実測: PC EEW 2 件 61px、スマホ 3 行 100px" },
  { id: "C8", title: "右パネルの縦の予算", rule: "detail (max-height:55%) + list (flex:1) + banner (flex:1 1 0) + credit + 設定/デモパネル", derived: "—", where: "style.css .side 周辺", risk: "設定・デモのパネルが開くと履歴が 0px まで潰れる。何を優先して残すかの規則が無い (出た順に押し出される)", ev: "実測 PC 1440×900 デモ + EEW: #list の高さ 0px" },
  { id: "C9", title: "上部バーの左右の境目", rule: "#sound{margin-left:auto} で右グループの開始位置を決める", derived: "—", where: "style.css `#sound`", risk: "ID が見た目の並びの境目を兼ねている。#sound が hidden のとき (配信) は境目が消える (ほかのボタンも隠すので実害は無い)", ev: "コード" },
  { id: "C10", title: "配信用 CSS ↔ 操作ボタンの ID", rule: ".broadcast #settings-open, #demo-open, #sound, #bgm, #overview, #calm-now, #back-live, #sound-hint を display:none", derived: "—", where: "style.css 末尾", risk: "操作系の部品を足すたびにこの一覧へ書き足す必要がある (部品に「操作系」の印が無い)", ev: "CSS" },
  { id: "C11", title: "別枠 ↔ 地図の CSS", rule: "<use href=\"#map-base\"> の複製には外部 CSS が効かないため、図形のスタイルは map.css を文字列として SVG の <style> に埋め込む。別枠が出ている間は .map.zoomed の規則が効かないので、寄ると別枠を隠す", derived: "—", where: "map.ts constructor / updateInsets()", risk: "図形のスタイルを CSS 変数やテーマで切り替えたい場合に制約になる (SVG 内 <style> は外のテーマを継承しにくい)", ev: "コード (コメントに理由あり)" },
  { id: "C12", title: "web ↔ native の二重管理", rule: "web の style.css / map.ts と native の draw.rs / frame.rs / panel.rs で、座標・寸法・色・文言を別々に持つ", derived: "別枠の範囲 (東経 122.5–131.5, 北緯 24–31) と高さ 220、凡例の 9 色、警報の帯の文、天気の札の向きの表など", where: "web/src/inset.ts のコメント (「native の frame.rs と同じ規則」) など、コメントで対応を示している", risk: "片方を直しても他方は自動では変わらない。両方を満たす共有仕様が無い", ev: "docs/nansei-inset.md が両方の同時変更を指示している" },
];

// ---------------------------------------------------------------------------
// レスポンシブ (ブレークポイント)
// ---------------------------------------------------------------------------
window.BREAKPOINTS = [
  { cond: "(既定)", target: "PC・タブレット横", effect: "grid: 1fr 380px。ページは viewport 高に固定、地図と右パネルは内側でスクロール。上部バーは 1 行", where: "style.css .layout / .side" },
  { cond: "@media (max-height:520px) and (min-width:801px)", target: "高さの低い PC 幅", effect: "出典を 1 行に切る / 詳細を max-height:40% / 凡例の行高を 10px に / 凡例の行間を詰める", where: "style.css" },
  { cond: "@media (max-height:440px) and (min-width:801px)", target: "横向きスマホ (幅 801px 以上)", effect: "お知らせ (#banner) を display:none", where: "style.css" },
  { cond: "@media (max-width:800px)", target: "スマホ縦・小さい画面", effect: "ページ全体スクロールに切替 (html,body{height:auto})。上部バー sticky + 折り返し、テロップを別の行 (order:10)、地図 60svh、右パネルは地図の下に縦積み、別枠を小さく (140/100px)、凡例を top:158px へ、時計・カウントダウンの位置と大きさを変更、お知らせは max-height:50vh", where: "style.css" },
  { cond: "@media (prefers-reduced-motion: reduce)", target: "動きを減らす設定", effect: "警報の帯・BGM の曲名の流し・予測の点滅・揺れの報告の輪のアニメーションを止める (計 4 か所)", where: "style.css / map.css" },
  { cond: "matchMedia (hover:none) and (pointer:coarse)", target: "指で触る端末", effect: "文言だけ「クリック」→「タップ」(dom.ts tapWord)。レイアウトには影響しない", where: "dom.ts" },
  { cond: "html.broadcast (?broadcast=1)", target: "配信用の Chrome", effect: "操作ボタン 8 個を display:none。音・BGM を最初から有効にする (localStorage へ書き込み)", where: "broadcast.ts / style.css 末尾" },
];

// ---------------------------------------------------------------------------
// 通信・永続化
// ---------------------------------------------------------------------------
window.DATAFLOW = [
  { kind: "WebSocket", path: "/ws", when: "常時 (切断時は 1 秒から 30 秒の指数で再接続)", consumer: "connection.ts → main.ts onEvents() → GroupStore / 津波 / 地震感知情報", feeds: "detail, list, 帯, 地図の塗り・震央・波, 時計 (server_time_ms で時刻同期), 音・読み上げ・通知" },
  { kind: "GET", path: "/api/warnings", when: "起動時 + 5 分ごと", consumer: "main.ts loadWarnings()", feeds: "warn 層, warn-banner, legend-warn, calm 判定の対象" },
  { kind: "GET", path: "/api/weather", when: "起動時 + 5 分ごと (サーバは 10 分ごとに取得)", consumer: "weather-layer.ts", feeds: "cities 層, rain 層, weather-caption" },
  { kind: "GET", path: "/api/telop", when: "起動時 1 回", consumer: "chrome.ts loadTelop()", feeds: "telop" },
  { kind: "GET", path: "/api/banners", when: "起動時 + お知らせが一巡するたび", consumer: "banner.ts", feeds: "banner (お知らせ)" },
  { kind: "GET", path: "/api/bgm", when: "起動時 1 回", consumer: "bgm.ts", feeds: "bgm ボタン, bgm-now (Icecast の status を別に取得)" },
  { kind: "GET", path: "/api/archive", when: "履歴の行を選んだとき", consumer: "history.ts fetchArchive()", feeds: "履歴再生 (demo.history)" },
  { kind: "GET/POST", path: "/api/tts/...", when: "読み上げが ON で新しい報が来たとき", consumer: "voice.ts", feeds: "音声 (見た目の部品ではない)" },
  { kind: "静的", path: "japan / areas / neighbors / tsunami / warning-areas (.geojson), stations.json, userquake-areas.json", when: "起動時 (warning-areas は初めて必要になった時)", consumer: "map.ts / main.ts", feeds: "地図の層, 観測点, 揺れの報告の位置" },
  { kind: "静的", path: "demo/index.json, demo/<id>.json", when: "デモを開いたとき", consumer: "demo-ui.ts", feeds: "demo-panel" },
  { kind: "localStorage", path: "eq-settings, eq-sound", when: "設定を変えたとき / 起動時に読む", consumer: "personal.ts, sound.ts", feeds: "settings-panel の各項目, 音 ON/OFF (配信は起動時に上書き)" },
  { kind: "URL", path: "?broadcast=1 / &sink= / &audio=mixer / ?demo=<id>", when: "起動時", consumer: "broadcast.ts / main.ts", feeds: "配信用の見た目・音の出力先・デモの自動開始" },
];

// ---------------------------------------------------------------------------
// native (配信) との対応
// ---------------------------------------------------------------------------
window.NATIVE = {
  frame: { w: 1280, h: 720, bar: 36, mapW: 900 },
  rows: [
    ["解像度", "可変 (viewport)", "1280×720 固定", "native は座標をすべて Rust の定数で持つ (draw.rs W/H/BAR_H/MAP_W)"],
    ["上部バー", "約 37px、折り返しあり", "36px 固定。題・版・[平時/地震]・BGM・配信元ラベル・状態の札", "状態の札 (混雑中・途切れ) と配信元ラベルは native だけ (docs/broadcast-status.md)"],
    ["地図の枠", "viewport − バー − 帯 − 右パネル (可変)", "x 0..900, y 36..720", "web は帯で押し下げ、native は帯を上に重ねる"],
    ["右パネル幅", "380px", "380px (x 900..1280)", "同じ"],
    ["南西諸島の別枠", "left10 top10 高さ 220 (幅 226)", "x10 y46 高さ 220", "範囲・大きさは同じ (docs/nansei-inset.md)"],
    ["小笠原の別枠", "あり (内容があるときだけ)", "なし", "未実装 (docs/nansei-inset.md で先送り。別の宿題)"],
    ["凡例", "54×238 まで可変 (行が増える)", "震度 34×147 + 警報 66×78 (平時のみ) を固定", "津波・P波S波の行は native に無い"],
    ["時計", "181×91 (PC)", "176×74", "右下。枠の色で接続状態を示す点は同じ。ただし web は 3 状態 (接続中・同期・切断中)、native は 2 状態 (panel.rs)"],
    ["天気の札", "絵文字 + 気温 (今/明日を交互)", "気象庁の SVG アイコン + 気温 (resvg で描く)", "札の向きの表 (SIDE / city_side) と避ける規則 (thin.ts / cards.rs) が別実装"],
    ["気象警報の帯", "押し下げ・横に流す", "上に重ね・2 行まで + 「ほか N 件」", "文と色の規則は同じ (warnings.ts ↔ native/data.rs)"],
    ["緊急地震速報の帯", "あり (最大 3 件)", "なし", "右パネルの EEW 詳細で代替"],
    ["津波の表示", "帯・沿岸線の色・凡例・一覧", "詳細に「津波」の文言のみ (音声は web と同じ規則)", "docs の N2 以降の予定のまま。f208e1e で確認済み (panel.rs・mod.rs)"],
    ["揺れの報告 (地震感知情報)", "輪の表示・トースト", "なし (音声の判断のみ)", "f208e1e で確認済み"],
    ["詳細", "スクロール・観測点一覧つき", "最大震度・震源・津波の固定レイアウト (区切り線 y=180)", ""],
    ["履歴", "最大 100 件・スクロール・クリックで再生", "最大 5 件", "操作が無いので再生なし"],
    ["お知らせ", "画像・文字・リンク", "文字のみ・最大 4 行 (y=492 の箱)", ""],
    ["出典", "静的 HTML", "CREDIT (6 行)", "文言が別々に書かれている"],
    ["設定・デモ・音・BGM ボタン", "あり", "なし (操作の UI を持たない)", ""],
    ["地震への寄り (ズーム)", "常時。自動カメラ + 手動のパン・ズーム", "enable_zoom / Frame::zoomed (設定で有効のとき)。都道府県の塗りのみで、細分区域は塗らない (eew.rs)", "web と同等ではない"],
    ["EEW の警報/予報の見出し札・予測最大震度札", "帯と詳細 (色分け)", "詳細の見出し札 (panel.rs) と、震央そばの「予測最大震度」札 (draw.rs)", "native は帯の代わりにこの 2 つで見せる"],
    ["早送り表示", "デモの操作パネル内", "時計の枠の中 (panel.rs)", "置き場所が違う"],
    ["のちに判明する震源 (履歴再生)", "薄い ✕ (scene.ts)", "draw_hindsight (hindsight.rs)", "両方にある"],
    ["テスト配信の帯", "なし", "あり (赤い帯。上端と下端)", "native だけ (test_mark.rs)"],
  ],
  // ワイヤーフレーム用の箱 (x, y, w, h)。[id, 表示名, region, x, y, w, h, 注記]
  boxes: [
    ["topbar", "上部バー", "topbar", 0, 0, 1280, 36],
    ["map", "地図", "map", 0, 36, 900, 684],
    ["side", "右パネル", "side", 900, 36, 380, 684],
    ["inset-okinawa", "南西諸島", "overlay", 10, 46, 226, 220],
    ["legend", "凡例 (震度)", "overlay", 10, 563, 34, 147],
    ["legend-warn", "凡例 (警報・平時のみ)", "overlay", 10, 479, 66, 78],
    ["clock", "時計", "overlay", 714, 636, 176, 74],
    ["detail", "詳細", "side", 900, 36, 380, 144],
    ["list", "履歴 (最大 5 件)", "side", 900, 181, 380, 300, "高さは推定 (お知らせ y=492 の手前まで)"],
    ["banner", "お知らせ (文字のみ)", "side", 916, 492, 348, 104, "最大 4 行 (推定の高さ)"],
    ["credit", "出典 (6 行)", "side", 900, 630, 380, 90, "推定"],
    ["warn-banner", "警報の帯 (重ね・2 行)", "banners", 0, 36, 1280, 45, "平時のみ。行数で 25〜45px"],
  ],
};

// ---------------------------------------------------------------------------
// 部品化への論点
// ---------------------------------------------------------------------------
window.OBSTACLES = [
  { n: 1, title: "配置が px の直書きで、部品どうしが暗黙に依存している", body: "別枠の寸法が、案内・カウントダウン・凡例 (スマホ) の位置に手計算で入っている (C1〜C4)。部品の並べ方を変えるには、この依存を「アンカーと積み方」に置き換えて消す必要がある。", sev: "高" },
  { n: 2, title: "描画が固定の ID への直書きで、部品を単位として扱えない", body: "描画関数は $(\"#id\") で DOM を直接引いて innerHTML / textContent / hidden を書く ($(\"#…\") か innerHTML を含む行を数えると web/src で 80〜90 (数え方で変わる。personal-ui.ts 19〜25・main.ts 15・view.ts 14・demo-ui.ts 13〜16・chrome.ts 10)。イベントも import 時に付く。部品を別の場所へ置く・複製する・無効にする単位が無い。", sev: "高" },
  { n: 3, title: "地図の描画と、地図の上の重ね物が同じクラスに同居している", body: "JapanMap が別枠・ツールチップ・矢印の層を #map へ append する (C6)。重ね物を地図から切り離して再配置できない。別枠は SVG の <use> 複製なので、地図の層構造に深く結びついている (C11)。", sev: "高" },
  { n: 4, title: "「どかし合い」が DOM セレクタ文字列に依存している", body: "天気の札は `.legend, .clock-panel, …` を querySelectorAll して避ける (C5)。部品が「ここは塞ぐ」と宣言する仕組みが無い。部品が増減・移動しても、避ける対象の一覧は自動では変わらない。", sev: "中" },
  { n: 5, title: "表示条件が複数のファイルに散っている", body: "hidden の書き換えが main.ts・view.ts・chrome.ts・weather-layer.ts・banner.ts・bgm.ts・personal-ui.ts・demo-ui.ts に分かれる。calmState() で一部は集約済みだが、部品ごとの可視性は述語として 1 か所に無い。", sev: "中" },
  { n: 6, title: "レスポンシブが幅 800px の単一ブレークポイントと高さの 2 段だけ", body: "横向きスマホ (幅 801px 以上) は PC の配置に落ち、実測 844×390 では別枠と凡例が重なる (C3)。タブレットなど中間の幅の専用の並びは無い。「通常版とモバイル版」を超えて増やすなら、プロファイルの考え方が要る。", sev: "中" },
  { n: 7, title: "native (配信) が別実装で、座標・色・文言を二重に持つ", body: "web と native の対応はコメントと docs の手動の取り決めだけ (C12)。レイアウトを部品の宣言として持つなら、native を対象に含めるかを先に決めないと、片方だけ進んで差が広がる。", sev: "中" },
  { n: 8, title: "レイアウトを検証するテストが無い", body: "web のテストは node --test の純粋関数のみ (DOM・ブラウザは使わない)。再配置で崩れても CI では分からない。今回の実測 (docs/ui-spec/tools/measure.mjs) は、座標のスナップショットを取る土台になる。", sev: "中" },
  { n: 9, title: "色の直書きが多く、部品単位のスタイルになっていない", body: "style.css の :root は 14 個の変数で、#rrggbb の直書きが 85 か所、var() の参照が 51 か所 (grep での概数。未精査)。震度の 9 色は index.html (凡例)・scale.ts・native の 3 か所に重複。部品ごとにスタイルを持たせるなら、トークンの整理が先に要る。", sev: "低" },
];

window.PROPOSAL = {
  contract: `// 部品の契約 (案。未決)
interface UiComponent {
  kind: string;                       // 部品の種別 ("clock" など)。インスタンス id (同じ部品を複数置く場合) と分ける
  region: "topbar" | "banners" | "map-overlay" | "side";
  // 表示条件: 状態から決まる純粋関数 (テストできる)
  visible(s: UiState): boolean;
  // 大きさの希望 (レイアウト側が配置と省略の判断に使う)
  size: { w?: number | "fill"; h?: number | "fill"; min?: { w?: number; h?: number } };
  // 「ここは他の部品に塞がせない」の宣言 (いまの thinCities の blockers を置き換える)
  keepOut?: boolean;                  // native の CardEnv.fixed (札を置かない固定矩形) と同じ考え方
  z: number;                          // 重なり順 (いまは CSS の z-index が部品ごとにばらばら)
  interactive: boolean;               // pointer-events と当たり判定の所有
  a11y: { role?: string; live?: "off" | "polite" | "assertive"; focusable?: boolean };
  states: ("normal" | "empty" | "loading" | "error")[];
  priority: number;                   // 場所が足りないとき、どれを残すか (縦の予算・札の置き場所)
  mount(host: HTMLElement): UiHandle; // 自分で DOM を作り、host の中に置く。$("#id") に頼らない
}
interface UiHandle { update(s: UiState): void; el: HTMLElement; destroy(): void }`,
  profile: `// レイアウトプロファイル (案。未決): 部品を「どの領域の、どの隅から、どの順で」置くかだけを宣言する
const desktop = {
  grid: "map 1fr | side 380px",
  topbar: ["title", "mode", "back-live", "overview", "calm-now", "wave-info", "telop", "|", "settings-open", "demo-open", "sound", "bgm", "bgm-now"],
  banners: ["eew-banner", "tsunami-banner", "warn-banner"],
  mapOverlay: {
    topLeft:     { stack: "column", items: ["inset-okinawa", "countdown"], beside: ["weather-caption"] },
    bottomLeft:  { stack: "column", items: ["legend"] },
    bottomRight: { stack: "column", items: ["inset-ogasawara", "clock"] },
    bottomCenter:{ stack: "column", items: ["tour-toast", "sound-hint"] },
  },
  side: ["settings-panel", "demo-panel", "detail", "list", "banner", "credit"],
};
const mobile = {                       // 幅 800px 以下 (縦長)
  grid: "single column, page scroll",
  topbar: ["title", "mode", "back-live", "overview", "|", "settings-open", "demo-open", "sound", "wrap: telop"],
  mapOverlay: {
    topLeft:     { stack: "row", items: ["inset-okinawa", "weather-caption"] },  // 別枠の横に案内
    bottomRight: { stack: "column", items: ["clock"] },
    bottomCenter:{ stack: "column", items: ["countdown", "tour-toast"] },
    // legend は地図の外 (地図の直下の細い行) に出す案もある
  },
  side: ["detail", "list", "banner", "credit"],   // 設定・デモは別のシート (開いたときだけ)
};`,
  steps: [
    { t: "0. 仕様を固める (本書)", d: "現状の部品・条件・結合を文書にする。実測のスクリプト (tools/measure.mjs) を残し、再配置の前後で座標を比べられるようにする。" },
    { t: "1a. 派生状態を 1 回だけ計算する", d: "tick の中で順序依存に作っている waving / feeling / userMoved などを UiState にまとめ、全部品へ配る。効果 (fetch・音・タイマー) は update から分ける。" },
    { t: "1. 表示条件を述語にする", d: "部品 id → (状態) => boolean を 1 か所 (例: visibility.ts) にまとめる。calmState() を拡張する形。DOM に触らないのでテストを書ける。この段階では見た目は何も変わらない。" },
    { t: "2. 部品のハンドルを作る", d: "$(\"#clock\") のような直接参照を、部品のレジストリ経由 (components.clock.update(state)) に置き換える。DOM の id は残して互換を保つ。import 時に付くイベントは mount() の中へ移す。" },
    { t: "3. 地図の重ね物を地図から切り離す", d: "JapanMap が作る別枠・ツールチップ・矢印を、地図の外の部品に分ける (別枠は <use> の参照先 #map-base を共有するだけにする)。" },
    { t: "4. 重ね物を「隅のスタック」で置く", d: "left/top/bottom の px 直書きを、隅ごとのスタック (縦または横) に置き換える。C1〜C4 の手計算がなくなる。keepOut の宣言で札の避け方も自動になる。実測で PC の現状と同じ座標になることを確認する。" },
    { t: "5. プロファイルの切り替え", d: "desktop / mobile (と必要なら landscape) を data-layout 属性または container query で切り替える。幅だけでなく、高さ・向きも条件に使う。" },
    { t: "6. native との共有 (要判断)", d: "プロファイルの宣言を JSON にして Rust も読む / 生成する案と、native は別実装のまま差分を表で管理する案がある。配信 (1280×720 固定) は desktop プロファイルの特別な場合と見なせる。" },
  ],
  questions: [
    "対象は web だけか、配信 (native) も同じ宣言で並べるか。",
    "モバイルの並びの方針: 地図の下に詳細・履歴 (今のまま) か、詳細を下からのシートにして地図を大きく見せるか。",
    "設定パネルとデモパネルは右パネルの一部のままか、モバイルでは別画面 (シート) にするか。",
    "横向きスマホ・タブレットを第 3 のプロファイルとして扱うか (今は PC 配置に落ち、重なりが出る)。",
    "部品の表示・非表示を利用者の設定にするか (例: 時計を隠す、凡例を畳む)。",
    "状況による自動の再配置 (例: EEW 中は詳細を地図の上へ) を許すか、並びは固定でプロファイルだけ切り替えるか。",
    "今回 web に無い部品 (native の状態の札・テスト配信の帯) を共通の部品として扱うか。",
    "参照したい画面 (YouTube の JDQ・JQuake のチャンネル。特に JDQ) の要素と並びを、どの程度まで取り入れるか。まず画面の要素の洗い出しが必要 (11.6)。",
  ],
};

// ---------------------------------------------------------------------------
// 確度
// ---------------------------------------------------------------------------
window.CONFIDENCE = {
  sure: [
    "web の DOM 構造・ID・クラスと、style.css / map.css の規則 (全文を読んだ)。",
    "部品の寸法と位置: Chromium (Playwright) で v0.31.0 をビルドし、5 つの幅・平時と EEW 中を実測した (付録 A)。",
    "データの取得経路・周期 (main.ts / chrome.ts / banner.ts / bgm.ts / weather-layer.ts の該当箇所)。",
    "ブレークポイントと、その中で変わること (style.css の @media 全 5 か所)。",
    "native に EEW の帯・津波の塗り・揺れの報告・小笠原の別枠が無いこと (レビューでも f208e1e で確認済み)。",
    "native の座標定数 (draw.rs / frame.rs / panel.rs / notice.rs / chip.rs / banner.rs の定数)。",
    "横向きスマホ 844×390 で別枠と凡例が重なること (スクリーンショットで確認)。",
  ],
  maybe: [
    "表示条件の表のうち「地震情報」「履歴再生」の一部のセル (レビューで telop・EEW の帯・カウントダウンの 3 行を修正済み。ほかにも同種の誤りが残りうる)。calmState / activeEews / priorityGroups の定義から読み取ったが、全状態の実機確認はしていない。",
    "native の履歴・お知らせ・出典の高さ (推定値と明記したもの)。",
    "小笠原の別枠の実測 (平時は出ない。値は CSS の読み取り)。",
  ],
  unknown: [
    "ブラウザ差: iOS Safari の svh・:has() の対応状況と、ノッチ (safe-area) まわりの挙動。",
    "実データでの崩れ: 長い警報文、同時に多数の EEW、観測点が多い大地震のときの詳細パネル。",
    "フォントの差による折り返し (今回の実測は Linux の Chromium)。Mac / iOS / Android での寸法は変わりうる。",
    "アクセシビリティの網羅 (フォーカス順・ランドマーク・スクリーンリーダー)。aria-live / role の付与は確認したが、操作順は未検証。",
    "配信 (?broadcast=1 の Chrome 描画) の実測。現在の本番は native 描画 (2026-09-30 に切替) とされるため対象外にした。",
    "プラグイン (RSS・Discord など) の出力の見た目は対象外。",
    "「履歴再生」は demo.history と selectedKey の 2 つの仕組みがあり、表示条件の表では 1 列にまとめている。",
    "JDQ は通常時・EEW (予報)・EEW (警報) + 震度速報・EEW 最終報 + 震度速報・地震情報 (確定)・2026/04/20 の EEW 警報 (第20・32・36報) の 8 枚、JQuake は通常時と EEW (予報→警報)・津波警報の発表直後とその 4 分後・能登半島地震 (EEW 3 件同時・日本海側の津波警報・大津波警報) の 11 枚のみ。警報 (赤)・津波予報・揺れの報告などの場面は未確認。",
  ],
};

// レビュー (Fable) で追加した設計上の論点
window.GAPS = [
  { t: "重なり順 (z-index)", d: "ツールチップ 3、案内・トースト・カウントダウン 2、その他の重ね物 1、モバイルの上部バー sticky 2。隅スタックに置き換えると衝突するので、部品に「層」(z) を持たせる。" },
  { t: "ポインタの所有 (pointer-events)", d: "重ね物はほぼ none、例外は画面外の矢印と音の案内。別枠は none なので中の震央が押せない。部品の契約に interactive と当たり判定を入れる。" },
  { t: "「避け合い」は DOM の実測に依存", d: "thin.ts は elementsFromPoint と getBoundingClientRect で測る。keepOut を宣言式にしても、陸・警報の判定は DOM 依存のまま。地図側が矩形・多角形を渡す形が要る。参考: native には既に「札を置かない固定矩形 + 置ける範囲」(card_room / CardEnv、警報帯の高さも除く) があり、keepOut はこの前例を web へ移す形で説明できる。" },
  { t: "render* の副作用", d: "renderWarnings が geojson を遅延取得、renderDemoControls が exitDemo を予約、updateBanner が fetch、updateBgm が音声再生。update(state) を純粋にするには、効果の分離が先。" },
  { t: "UiState の定義と派生状態", d: "tick の中で、順序のある派生値 (waving → 凡例・テロップ、feeling → calmState、userMoved → モード) を計算している。「派生状態を 1 回計算して全部品へ配る」段階を手順に入れる。" },
  { t: "起動順の前提", d: "broadcast.ts が localStorage を書いてから loadSettings が動く (main.ts 先頭の import 順)。mount() 化するとこの前提が壊れうる。" },
  { t: "配信用 web (?broadcast=1) のプロファイル化", d: "desktop と同じ並びで interactive=false の第 3 プロファイルにすると、C10 (配信用 CSS の ID 列挙) が不要になる。" },
  { t: "部品の複製と DOM id", d: "「id = DOM id」の方針は、同じ部品を 2 か所に出す案 (凡例を地図の外にも、など) と矛盾する。インスタンス id と部品の種別を分ける。" },
  { t: "空・読み込み中・エラーの状態", d: "詳細は 6 通りの内容を持つ。部品の契約に、通常以外の状態 (empty / loading / error) を含める。" },
  { t: "更新頻度と動きの分類", d: "rAF (波)・1 秒・200ms (デモ) と、CSS アニメーション (拍動・点滅・パルス) が混在。reduced-motion と配信 (ヘッドレス) で止めたい一覧を、部品側の属性として持つ。" },
  { t: "アクセシビリティ", d: "aria-live は詳細=polite・EEW の帯=role alert・テロップ=off・時計=off、凡例・案内は aria-hidden。再配置で live region の順が変わる影響、フォーカス順、モバイルのシート案でのフォーカストラップ、地図のキーボード操作 (現状なし) を、契約の項目 (role / aria-live / focusable) にする。" },
  { t: "「決めない」ことの明記", d: "safe-area (env(safe-area-inset-*) は未使用、時計は bottom:6px)、iOS の svh、テーマ (color-scheme は dark 固定、SVG 内 <style> は外の変数を継承しにくい: C11)、多言語 (文言が TS と Rust に直書き)。やらないなら、やらないと書く。" },
  { t: "テストの置き場所", d: "純粋な visible(state) は node --test、座標は measure.mjs のスナップショット。native には位置のテスト (frame.rs 等) が既にあるので、web 側にも DOM を使うテスト (happy-dom など) の置き場を提案に入れる。" },
  { t: "地図が単一インスタンス", d: "dom.ts が JapanMap を 1 つだけ作り (export const map)、重ね物を探す処理もページ全体の querySelectorAll に頼る。JDQ のように全国図と寄りの図を同時に出すなら、地図を複数持てる設計 (インスタンスごとの重ね物・カメラ) が要る。" },
  { t: "性能", d: "thinCities はカメラの更新 (applyView) のたび = パン・ズームの毎フレーム走る。renderList は最大 100 行を innerHTML で作り直す。再配置で ResizeObserver → applyView → thinCities の連鎖が増える点を C7 に足して見る。" },
];

// JDQ (YouTube 配信) の画面の構成。利用者が共有した 1 枚のスクリーンショット (1920×1080 相当) から読み取ったもの。
// 画像そのものはリポジトリに入れていない。通常時、EEW (予報) 発表中、EEW (警報 第23報) + 震度速報、EEW 最終報 + 震度速報、地震情報 (震源確定・各地の震度)、2026/04/20 の EEW 警報 (第20・32・36報、津波予報の発表直後) の計 8 枚から。確定した各地の震度の続き・津波予報の続き・揺れの報告などほかの場面は未確認。
window.JDQ = {
  summary: [
    "16:9 固定の「ペインの格子」: 上段は左右 2 つの大きなペイン (左: 全国の揺れ監視と EEW / 右: 最新の地震へ寄った地図)。下段は左に時計・ライブカメラ、右に地震の一覧・気象警報・天気・雨雲レーダー。",
    "各ペインは「枠線 + 見出し」を持ち、枠の色が役割を示す (左=緑、右=青)。",
    "詳細 (最大震度・震源・M・深さ・発生時刻・津波の有無) は、右パネルではなく、寄った地図の上に大きく重ねている。",
    "同時に 2 枚の地図 (全国 + 寄り) を出している。",
  ],
  rows: [
    ["全国ペイン (見出し「リアルタイム揺れ監視と緊急地震速報」・緑枠)", "上段左", "地図 (全体図) + EEW の帯", "見出し付きの枠 (ペイン) という概念が無い。「緊急地震速報は発表されていません」の状態文は、帯の代わりにペイン内へ常設", "要"],
    ["観測点の点群 (全国に多数。色は揺れの強さ)", "全国ペインの地図", "なし (震度観測点は寄ったときだけ点で出す)", "リアルタイムの揺れ (強震モニタ系) のデータ源が要る。現行のデータ源 (P2P地震情報・Wolfx) には無い。別途の調査が必要", "要"],
    ["計測震度のゲージ (−3〜7) と「強震最大震度」「最大加速度」の枠", "全国ペインの左", "凡例の震度ゲージ (1〜7 の離散)", "連続値のゲージと、最大値の欄は無い。データ源も同上", "要"],
    ["地震情報履歴 直近 20 件 (地図の右下に重ねる)", "全国ペインの上", "履歴の一覧 (右パネル)", "履歴を「地図の上の重ね物」として置く = 一覧の部品が領域をまたいで置けること", "中"],
    ["最新地震ペイン (見出し「最近の地震情報と緊急地震速報」・青枠)", "上段右", "地図 (カメラが寄る) + 詳細パネル", "うちは 1 枚の地図がカメラで寄る。JDQ は全国と寄りを同時に出す → 地図を 2 つ持てる必要。いまの JapanMap は単一 (dom.ts の map)", "確"],
    ["大きな最大震度・震源名・M・深さ・発生時刻・「津波の心配はありません」", "最新地震ペインの上に重ねる", "詳細パネル (右パネル) + 詳細内の津波の文", "詳細を地図の重ね物として置けること。置き場所が領域に依存しない部品が要る", "中"],
    ["「試験放送」の札・版の表示 (β0.61-draft01)", "最新地震ペインの右上", "native のテスト配信の帯 / 版の表示", "web には試験用の表示が無い (native にはある)", "中"],
    ["直近の地震リスト (大きな最大震度の札 + 震源 + M + 時刻、<b>7 件</b>)", "下段右", "履歴 (native は 5 件、web は最大 100 件)", "表示件数が少ない大きな札の別バリエーション。同じ部品で見た目を変えられるか", "中"],
    ["気象警報のパネル (都道府県・区域・種別・段階チップ・発表時刻)", "下段右の中", "気象警報の帯 + 地図の塗り + 凡例", "うちは帯 + 塗り。JDQ は専用のパネル (段階チップの凡例つき)", "中"],
    ["都市の天気カード (1 都市: アイコン・最高/最低)", "下段右", "地図上の天気の札 (複数都市)", "表示の形が別 (地図の外の独立カード)", "中"],
    ["雨雲レーダー (全国)", "下段右", "なし (アメダスの雨の点のみ)", "別のデータ源 (気象庁の降水ナウキャスト等) が必要。部品の追加", "要"],
    ["「最近の大きな地震」の帯 (日時・最大震度・震源・M・深さ)", "下端", "なし (履歴の震度の下限設定で代替できる範囲)", "最大規模の地震を 1 行で常設する部品", "中"],
    ["時計: アナログ + デジタル (日付・時刻・秒) + 「NTP server」の同期表示", "下段左", "時計 (デジタルのみ。接続状態は枠の色と文言)", "アナログ時計は無い。同期状態の表示の考え方は同じ", "低"],
    ["配信元のロゴ (JDQ / Twitch)", "下段左", "native の配信元ラベル (label)", "ロゴ画像の枠", "低"],
    ["[EEW 中] 見出し札「緊急地震速報 (予報)」「最終報」(橙) + 大きな「予想最大震度」(黄の札) + 震源名・発生時刻・M・深さ", "全国ペインの左上と、最新地震ペインの左上の<b>両方</b>", "EEW の帯 (横長の 1 行) + 詳細パネル (右パネル)", "うちは帯 (1 行の文) と右パネルの詳細に分かれる。JDQ は同じ内容の大きな札を、<b>各地図の上に重ねて 2 か所</b>に出す (右ペインの札には発生時刻が無く、版によっては下端の帯に「発生時刻・#報番号」)。予報=橙は うちと同じ考え方 (うちも予報は橙)。警報 (赤?) の見た目は未確認", "確"],
    ["[EEW 中] P波・S波の円 (橙の塗り + 細い円) を<b>全国図と寄り図の両方</b>に描く", "両ペインの地図", "P波・S波の円 (地図 1 枚)", "地図が 2 枚あるので波も 2 回描く。寄り図では円が画面の大半を占める。うちの別枠は波も複製して描く (<use>) が、2 枚目の地図の概念は無い", "確"],
    ["[EEW 中] 観測点の点群が強震モニタの震度で色づく (リアルタイムの揺れ)。県別の「強震モニタ震度」リスト (4 茨城県・4 埼玉県…) を地図の右下に重ねる", "全国ペイン", "なし。うちは予測・観測の震度で都道府県を塗る", "<b>見せ方が違う</b>: JDQ は都道府県を塗らず、観測点の点と円で見せる。うちは塗りが主。リアルタイムの揺れのデータが無いと再現できない (要データ源)", "要"],
    ["[EEW 中] 「強震最大震度」「最大加速度」の枠に値が入る (黄)", "全国ペインの左", "なし", "平時は空の枠で、揺れの間だけ値が入る (常設の枠)。データ源は同上", "要"],
    ["[EEW 中] 気象警報のパネル・天気カード・雨雲レーダー・履歴 7 件・「最近の大きな地震」の帯は<b>そのまま出続ける</b>", "下段右", "うちは平時 (calm) だけ警報・天気を出し、地震の間は消す", "<b>状態で出し入れしない</b>。JDQ は常設の領域に、状態に関わらず出す (画面の構成が変わらない)。うちは状態で部品を出し入れして、構成が変わる。どちらの流儀にするかが、部品の契約 (表示条件) の核心", "確"],
    ["[震度速報] 右の寄り図ペインが「最大震度 5弱」「<b>震源調査中</b>」「震度速報 23日02時00分ごろ発生」に切り替わる。地図には区域ごとの<b>震度の数字札</b> (4・3・5−) と、震度の凡例 (5弱/4/3)。右下に「津波の影響 調査中」", "最新地震ペイン", "詳細パネルの「震源調査中」・「津波の有無を調査中」 + 地図の震度の数字札", "<b>文言がほぼ同じ</b> (震度速報・震源調査中・津波の有無を調査中)。数字札も同種 (うちは都道府県・細分区域の内側に置く)。JDQ の右ペインは、震度速報が届くと確定情報に切り替わる (EEW 中は EEW を映す: 下の 2026/04/20 の行)", "確"],
    ["[警報 + 震度速報] 右下の領域に、放送風の大きな「緊急地震速報 (気象庁)」パネル (赤い見出し・青地の本文「茨城県で地震 強い揺れに警戒」・地域名の列・左にミニ地図 (警戒地域を黄で塗る))。<b>台風情報・気象警報・天気・雨雲の領域を覆う</b>(下端に少しだけ見える)", "下段右", "EEW の帯 (横長 1 行。「強い揺れに警戒: 宮城県・岩手県」)", "うちの帯と近い内容 (見出し + 一文 + 地域名) を、<b>大きなパネルとして領域を奪って</b>出す。予報 (09/29) の画面では履歴などが出続けていたので、警報のときだけ奪う可能性がある (要確認)", "中"],
    ["[警報] 全国図の S 波の円が<b>赤い大きな塗り</b> (予報は橙)。左の枠の札は赤。「強震最大震度」「最大加速度」の枠が黄で埋まる", "全国ペイン", "S 波の円 (赤の線 + 薄い塗り)", "円の色が警報=赤・予報=橙で変わる。うちは P 波=青・S 波=赤で、警報/予報では変えない", "確"],
    ["[震度速報] 下端の帯: 種別ラベル「震度速報」+ 文「今後の情報に注意してください」(青)", "下段右の下端", "テロップ (平常時のみ。EEW 中は消す)", "<b>状態に応じた案内文を常設の帯に流す</b>。種別ラベル付き。うちは地震の間はテロップを消す作り", "確"],
    ["[最終報 + 震度速報] 右下の大きな EEW パネルが<b>消え</b>、履歴の代わりに「<b>台風情報</b>」(縦書きの見出し + 台風の進路図)、気象警報パネル (沖縄県 石垣市 大雨・土砂災害 …)、天気カード、雨雲レーダーが並ぶ", "下段右", "なし (台風情報)。気象警報は帯 + 塗り", "EEW パネルの占有は<b>一時的</b> (第 23 報 02:02:41 では出ていて、最終報 02:03:31 では消えている)。<b>台風情報</b>は新しい部品 (データ源は気象庁の台風情報で、現行には無い)。この時間帯の右下に履歴が見えないので、領域の中身が時間で入れ替わる (巡回する) 可能性もある (要確認)", "中"],
    ["[最終報 + 震度速報] 下端の帯が「震度速報」ラベル + 「震度5弱 茨城県北部 茨城県南部 埼玉県南部 千葉県北西部」(区域の列挙) に変わる", "下段右の下端", "テロップ / 詳細の観測点一覧", "同じ帯に、状態ごとの内容 (案内文 → 震度別の区域列挙) が入る。うちの観測点の一覧 (<details>) に当たる内容を、帯に流す形", "確"],
    ["[最終報] 「強震最大震度」の枠が黄から<b>緑</b>へ (揺れの強さに追従して色が変わる)", "全国ペインの左", "なし", "リアルタイムの最大値を色で表す枠。値が下がると色も戻る。データ源は同上", "要"],
    ["[地震情報 (確定)] 右の寄り図ペインに「最大震度 5弱 / <b>茨城県南部 M5.9 深さ70km</b> / 地震情報 23日02時00分ごろ発生」(震源が確定)、観測点ごとの<b>丸い震度の点</b> (青 2・緑 3・黄 4・5−) が震央の周りに密集、震度の凡例 (5弱/4/3/2/1)、「津波の心配はありません」", "最新地震ペイン", "詳細パネル + 震度の塗り (都道府県・細分区域) + 観測点の点 (寄ったときだけ)", "<b>うちの「地震情報 (各地の震度)」の見せ方に最も近い</b> (震源・M・深さ・最大震度・津波の文言・観測点の点・凡例)。違いは、JDQ は塗りより「点 + 数字」を主役にする点と、寄り図が関東全体を映す (うちは揺れた区域の外接矩形へ寄る) 点", "確"],
    ["[地震情報 (確定)] 全国ペインが「緊急地震速報は発表されていません」に<b>戻り</b>、地図の右下に「地震情報履歴 直近20件」が<b>重なる</b>。EEW の最終報 (02:03:31) から 3 分 20 秒後 (02:06:51) には戻っている", "全国ペイン", "履歴の一覧 (右パネル) / 平時への復帰 (settle)", "EEW が出ている間は履歴の重ね物を隠し、終わると出す (EEW の札・円と領域を共有)。<b>復帰は最終報から 3 分 20 秒以内</b> (うちの EEW の帯は受信から 3 分 = EEW_BANNER_MS。近い)", "確"],
    ["[地震情報 (確定)] 下端の帯が「地震情報」ラベル + 「震度5弱 筑西市 つくばみらい市 川口市 蕨市 戸田市 宮代町 浦安市 印西市」(<b>市区町村・観測点名の列挙</b>)", "下段右の下端", "詳細の観測点一覧 (<details>、震度別)", "同じ帯に、震度速報では区域名、地震情報では観測点名を列挙して流す。うちの「震度別の観測点一覧」に当たる内容を帯にする", "確"],
    ["[台風が存在する時間帯] 右下の領域が「<b>台風情報</b>」(縦書き見出し + 進路図) + 気象警報 (沖縄県) + 天気カード (<b>宇都宮 → 秋田に切り替わる</b>) + 雨雲レーダー。履歴のリストは出ていない", "下段右", "なし", "領域に入る部品が状況で変わる (台風がある日は台風情報、無い日 (10/05) は直近の地震リスト 6 件)。<b>候補の部品に優先度を付けて領域へ入れる</b>形に見える (要確認)。天気カードの都市は一定時間で巡回する (うちの天気の札は 13 都市を同時に出す)", "中"],
    ["[2026/04/20 三陸沖 M7.7 の EEW 警報 第20報 (16:53:32)] <b>右ペインが EEW を映す</b>: 大きな予想震度の札 (4・黄) + 「三陸沖で地震」+ 規模 M6.8・深さ 30km。地図には予想震度 4 以上の<b>細分区域を黄で塗り</b>、P波 (青) と S波 (赤) の円・震央 ✕。右下に凡例 (予想震度4以上 / S波 / P波 / 震央)、下端に見出し札「緊急地震速報 警報」「04/20 16:52:57発生」「#20 (報の番号)」", "右ペイン (最新地震ペイン)", "地図 (予測の塗り・P/S 波の円・震央) + 詳細パネル + 凡例の P波・S波の行", "<b>うちの EEW の見せ方とほぼ同じ構成</b> (予測の塗り・P/S 波・震央・報の番号・凡例にも P波/S波の行)。つまり JDQ の右ペインは「いま見せる地震」(EEW でも確定情報でも、新しい方) で、うちの <code>currentGroup()</code> に相当する。左の全国ペインは常に「ライブの揺れ」を担当", "確"],
    ["[同 16:53:32] 右下に「緊急地震速報 (気象庁)」パネル (赤見出し・青地・「三陸沖で地震 強い揺れに警戒」・地域名の列・ミニ地図) と、その下に<b>予想震度別の区域リスト</b> (4: 青森県下北 / 岩手県沿岸北部 / 岩手県沿岸南部 …)", "下段右", "EEW の帯 + 詳細パネルの区域一覧 (予測震度つき)", "うちの EEW 詳細の「予測の区域の一覧」(震度の札 + 区域名) と同じ内容。右下の領域がパネルに占有される様子は 08/23 と同じ", "確"],
    ["[同] 全国ペイン: 震央へ寄る (東北が大きく映る)、S波は<b>赤い塗りの円</b>、P波は<b>緑の細い円</b>、観測点の点が黄緑に色づく", "全国ペイン", "地図のカメラ + P波 (青)・S波 (赤) の円", "円の色の割り当てが違う (JDQ: P=緑・S=赤塗り / 右ペインは P=青・S=赤)。同じ JDQ でも、ペインごとに色が違う", "確"],
    ["[同 16:54:39 EEW 第32報 + 震度速報] 左の EEW 札は M6.8 → <b>M7.5</b>・予想最大震度 4 → <b>5強</b>に更新。右ペインは EEW から<b>震度速報へ切り替わり</b>、「5強 / 震源調査中 / 震度速報」「2026/04/20 16:53ごろ」「<b>津波の有無は調査中</b>」。地図は区域の震度札 (5強・5弱・4)、右下の凡例は震度 5強〜1 の全段階の札 + 震央", "右ペイン + 左ペインの札", "詳細パネル (「震源調査中」「津波の有無を調査中」) + 地図の数字札", "文言も構成もうちとほぼ同じ (津波の有無を調査中 = TSUNAMI_TEXT の Checking)。EEW の報は 12 報で M・震度が大きく上がっており、うちの「報ごとの最大を保つ」(v0.23.0) と合わせて、札の値が報ごとに変わる表示の再現が要る。右ペインの凡例は全段階のチップで、うちの縦ゲージとは形が違う", "確"],
    ["[津波予報の発表直後 16:55:43 (EEW 第36報 の最中)] 全国ペインの左上、EEW 札の下に「<b>津波到達予想</b>」パネルが<b>重なる</b>: 黒い見出し + 「津波警報」(赤の札) の行 (北海道太平洋沿岸中部 午後5:30 <b>3m</b> / 岩手県 <b>すぐ来る</b> 3m) + 「津波注意報」(黄の札) の行 (北海道太平洋沿岸東部・西部 5:30 1m / 青森県太平洋沿岸・宮城県 5:20 1m)。パネルは計測震度のゲージを覆う", "全国ペイン (左上)", "津波予報の帯 (種別ごとに区域名を並べる) + 詳細パネルの区域行 (「予想 3m」「直ちに来襲」) + 凡例の津波の等級", "<b>津波は帯でなく、地図の上の重ね物 (表)</b>。種別 (警報=赤・注意報=黄) の札 → 区域名 → 到達予想時刻 → 予想高さの 4 列。うちは同じ情報を、帯 (区域名のみ) と詳細 (高さ・直ちに来襲) に分けている。(<b>この画像は、レビューの時点で保存されておらず第三者が検証できなかった。私が画像を直接見て読み取った内容</b>) データ項目は <code>TsunamiArea</code> (grade / immediate / first_height / max_height) にあるが、<b>到達予想時刻</b>が別の項目か first_height の文字列に入っているかは未確認 (要確認)", "確"],
    ["[同] このとき右ペインはまだ震度速報のまま「津波の有無は調査中」、右下は EEW パネル。右下の下端に流れる文字 (縦書きの断片: 「笠原」など = 小笠原の津波に関する案内?)", "右ペイン / 下段右", "—", "津波予報 (552) が届いても、右ペインの「津波の有無」の欄は震度速報の情報で決まる (<b>情報の種別ごとに更新される場所が違う</b>)。うちの詳細の「津波」の行 (地震情報の中身) と同じ。下端の流れる文字は未確認", "中"],
    ["[版の違い] この日 (2026/04/20) の画面は、ロゴが「<b>JDQ1</b> 地震速報」、ペインの見出しが<b>塗りのバー</b> (緑・水色)、カメラが渋谷区の別の映像。08/23 以降 (β0.60・0.61) は「JDQ Twitch」ロゴと<b>枠線 + 見出し</b>", "全体", "—", "見た目は版で変わるが、<b>要素の構成 (左: ライブの揺れ / 右: いまの地震 / 右下: 情報パネル / 下端: 帯 / 左下: 時計とカメラ) は連続</b>している。参照にするのは構成で、見た目の細部ではない", "確"],
    ["別枠 (南西諸島) と、右上の ⚡◀ アイコン、寄り図の出典「地図データ: 気象庁, Natural Earth」", "全国ペインの左 (状態枠の下・計測震度ゲージの右) / ペインの右上 / 寄り図の左上", "別枠: 南西諸島 / なし / 出典 (右パネルの下)", "<b>JDQ にも南西諸島の別枠がある</b> (寄っている間は消える点も同じ)。⚡◀ は JQuake と同種のアイコンで意味は未確認 (EEW の状態を示すものに見える)。地図ごとの出典表記は、サブの地図を持つ部品が自分で持つ", "中"],
    ["ライブカメラ枠 (東京 渋谷・水槽など)", "下段左", "—", "対象外 (利用者の意向: 水槽などは取り入れない)", "—"],
  ],
  takeaways: [
    "JDQ の見せ方は <b>web の通常版・モバイル版の延長ではなく、配信用 (1280×720 / 1920×1080) の別プロファイル</b>として設計するのが自然。「ペインの格子」を宣言できる必要がある。",
    "部品を「領域 (地図 / 右パネル) に属するもの」ではなく、<b>置き場所を選べるもの</b> (詳細・履歴が地図の上にも右パネルにも置ける) にしておくと、JDQ 型の並びに近づける。",
    "<b>地図を 2 つ同時に持てること</b> (全国 + 寄り) が、現状の最大の構造上の障壁。JapanMap の単一インスタンス前提 (dom.ts の <code>map</code>、<code>document.querySelectorAll</code> で重ね物を探す thinCities など) をほどく必要がある。",
    "<b>JDQ の右ペイン = うちの <code>currentGroup()</code></b>: 09/29 (予報) と 04/20 (警報) では EEW を映し、08/23 (震度速報が届いたあと) では確定情報を映す。「新しい方 / 優先度の高い方」を右ペインに出す規則で、うちの優先度の規則 (priority.ts) と同じ考え方。一方、JQuake の右上は確定した地震のまま固定で、EEW には反応しない (別の流儀)。",
    "<b>JDQ の流れ (EEW → 震度速報 → 地震情報) は、うちの状態遷移とよく対応</b>する: EEW 中 (全国ペインが主役) → 震度速報 (右ペインが「震源調査中」+ 区域の震度) → 地震情報 (震源確定・観測点の点) → 約 3 分で全国ペインの状態文へ復帰。うちの <code>calmState</code> / <code>EEW_BANNER_MS</code> の時間感覚とほぼ同じで、遷移の規則を作り直す必要は小さそう。",
    "<b>JDQ の右下は、警報のとき複数の部品の領域を 1 つのパネルが<u>一時的に</u>「奪う」</b>ように見える (履歴・警報・天気・雨雲 → 大きな EEW パネル → 最終報では元に戻る)。常設領域の流儀の中でも、「領域の上書き (ある部品が、状態によって別の部品の領域を一時的に占有する)」という仕組みが要る。予報の画面では奪っておらず、警報の最終報でも戻っているので、条件は報の種別・報の更新直後・時間に依る可能性がある (要確認)。",
    "<b>JDQ は「常設の領域 + 中身が変わる」流儀</b>: 地震が来ても画面の構成は変わらず、各ペインの中身 (EEW 札・波・震度) が入れ替わるだけ。うちは「状態で部品を出し入れ」する (平時だけ警報・天気を出す、など)。どちらにするかで、表示条件 (visible) の設計が変わる。常設領域を採るなら、部品は「領域」と「その中の表示状態 (空・平時・EEW 中)」を持つことになる。",
    "データ面の追加が要るもの: リアルタイムの観測点の揺れ・計測震度・最大加速度 (強震モニタ系)、雨雲レーダー。レイアウトの前に、データ源の可否を調べる必要がある。",
  ],
};

// JQuake (YouTube 配信) の画面の構成。利用者が共有した 1 枚のスクリーンショットから読み取ったもの (画像はリポジトリに入れていない)。
// 利用者の所感: 「今の私達のに近いが、右上が細かい表示になっている」「音は無いが、同様に細かく出す。データが入ったら細かに色付けで反応する」
// 通常時 2 枚 + EEW (予報 第1報 → 警報 第4・11・14報) 4 枚 + 2026/04/20 の EEW 警報 第36報 (津波警報の発表直後) と、その約 4 分後 (16:59:59、津波警報が出ている間) の計 2 枚、さらに 2024/01/01 能登半島地震の 3 枚 (EEW 警報 第35報の 3 件同時、16:18:42 日本海側の津波警報、16:23:17 大津波警報)、計 11 枚から。確定した地震の観測震度の表示・津波の続き・揺れの報告などは未確認。
window.JQUAKE = {
  summary: [
    "2 カラム: 左が全国の大きな地図 (約 2/3)、右が縦の列 (約 1/3)。うちの「地図 + 右パネル」に近い。",
    "右列の上に、<b>震源が確定している間は</b>「最新の地震へ寄った小さな地図」があり (確定した震源が無い間は地図なしで見出しだけ。2024/01/01 の画面)、日時・津波の有無・最大震度・震源名・深さ・M を<b>地図の上に重ねて</b>いる (ここが「右上の細かい表示」)。凡例 (① 震度 / ✕ 震央) と出典 (地図データ: 気象庁, Natural Earth) も地図の右下・左下に重ねる。",
    "その下に、地震の履歴を大きな行で縦に並べる (日時 + M + 震源名 + 右端に大きな最大震度)。うちの履歴 (最大 100 件の細い行) より行が大きく、6〜7 件 (画面の高さで変わる)。",
    "左の地図の上には、状態文 (「緊急地震速報は発表されていません」) の大きな枠、計測震度のゲージ、南西諸島の別枠、付近の観測点のデータ、日時を重ねる。下端に青い帯 (案内文)。",
  ],
  rows: [
    ["左: 全国の地図 + 観測点の点群 (色は計測震度) と地名", "左カラム全体", "地図 (全体図)", "観測点の点群はリアルタイムの揺れのデータ (強震モニタ系) で、現行のデータ源に無い (JDQ と同じ)。地名の常時表示も無い", "要"],
    ["状態文の枠「緊急地震速報は発表されていません」 (左上の大きな半透明の枠)", "地図の左上", "EEW の帯 (出ているときだけ、上部バーの下)", "うちは「出ていないときは何も出さない」。JQuake は常設の枠で、EEW が出るとここに内容が入る形に見える (他の場面は未確認)", "中"],
    ["計測震度のゲージ (−3〜7、縦)", "地図の左", "凡例の震度ゲージ (1〜7 の離散。左下)", "連続値のゲージ。位置は左中央で、うちは左下", "中"],
    ["南西諸島の別枠", "地図の左 (状態文とゲージの間)", "別枠: 南西諸島 (左上)", "近い。位置と並び (状態文・ゲージとの位置関係) が違う", "確"],
    ["付近観測点データ (観測点名・震度と加速度の棒)", "地図の右下", "なし", "観測点を選んで、その場の値を出す部品。データ源は同上", "要"],
    ["日時 (大きなデジタル、秒まで)", "地図の右下", "時計 (右下の枠)", "ほぼ同じ位置。うちは接続状態を枠の色で示す点が違う (JQuake は日時のみ)", "確"],
    ["下端の青い帯 (案内文: 「JQuake放送へようこそ…」)", "地図の下端", "テロップ (上部バー) / お知らせ (右パネル)", "案内を「地図の下端の帯」として置く。うちは別の場所に別の部品で出している", "中"],
    ["左下の操作アイコン (現在地・設定)", "地図の左下", "上部バーの「設定」ボタン + 設定パネル", "操作ボタンを地図の上の小さなアイコンにしている", "中"],
    ["右上のアイコン (⚡ ▶)", "地図の右上", "なし", "意味は未確認 (EEW の状態を示すアイコンに見える)", "要"],
    ["[EEW 中] 左上の状態文の枠が、見出し札「緊急地震速報 (予報/警報)」+「第 N 報」に<b>置き換わる</b>。予想最大震度の大きな札 (黄・橙など震度の色) + 震源名 + 発生時刻 + M + 深さ", "地図の左上 (状態文と<b>同じ枠</b>)", "EEW の帯 (横長 1 行) / 詳細パネル", "<b>常設の枠に中身が入れ替わる</b>。予報は橙、警報は赤の札で、報が進むと M・深さが更新される (第 1 報 M5.0・30km → 第 4 報 M6.1・50km → 第 14 報 M6.3・80km。<b>発生時刻も報ごとに変わる</b>: 02:00:44 → 42 → 40 → 39)。うちは報ごとに最大を持ち続ける作り (v0.23.0) で、値の動き方が違う", "確"],
    ["[EEW 中] 左の全国図が<b>震央へ自動で寄る</b> (関東が大きく映る)", "左カラムの地図", "地図のカメラ (EEW で震央・揺れる地域へ寄る。web・native とも)", "同じ考え方。うちの「1 枚の地図が寄る」は、JQuake の左の地図に相当する", "確"],
    ["[EEW 中] 予想震度の区域を段階色で塗る (青 2・緑 3・黄 4・橙 5−) + 観測点の点が<b>時間とともに色づく</b> (青 → 黄緑 → 黄) + 震央の ✕ と細い円 (S 波)", "左の地図", "予測の塗り (都道府県・細分区域) + P波・S波の円", "塗りは同じ考え方。違いは<b>観測点の点が「いまの揺れ」で反応する</b>こと (データ源が要る)。JQuake も P波 (青緑の大きな円)・S波 (赤の円。広がると画面の大半を占める赤い塗りの円) の 2 本 (第 1 報の時点では P 波のみ)", "中"],
    ["[EEW 中] 「付近観測点データ」の色 (震度・加速度の棒) が、青 → 黄へ変わる", "地図の右下", "なし", "データが入ると色で反応する部品。入力 (観測点のリアルタイム値) が要る", "要"],
    ["[EEW 中] 右上の寄り地図は<b>変わらない</b> (直近に確定した地震のまま)。履歴・下端の帯・日時以外の部品も変化なし", "右カラム全体", "—", "<b>右上の地図は「最新の確定した地震」を固定で見せる役</b>で、EEW には反応しない。EEW は左だけが受け持つ。2 枚目の地図は、カメラ追従の無い固定ビューで足りる可能性がある", "確"],
    ["履歴の行の色: 震度 1 は灰、震度 2 は青、(それ以上は未確認)", "右列の下", "履歴の行 (震度の小さなバッジのみ)", "行全体を震度で塗る見せ方。うちは行頭の小さな札だけ", "中"],
    ["[2026/04/20 16:55:41 EEW 警報 第36報 + 津波警報の発表直後] EEW の札のすぐ下に、小さな赤い札「<b>津波警報発表中</b>」。全国図の<b>海岸線が等級の色で縁取られる</b> (岩手〜青森の太平洋側と北海道の太平洋側が赤、その外側・千島側が黄)", "地図の左上 (EEW の札の下) + 全国図の沿岸", "津波予報の帯 (区域名つき) + 沿岸線の等級の色 + 凡例", "<b>うちの津波の見せ方に最も近い</b>: 沿岸線を等級で色分け (うちは警報=赤・注意報=黄・大津波警報=紫)。違いは、区域名を並べる帯の代わりに、<b>「津波警報発表中」の小さな札 1 つ</b>にしている点。JDQ (到達予想の表) と比べると、JQuake は情報量を絞って地図の色に任せる", "確"],
    ["[2026/04/20 16:59:59 津波警報が出ている間] 左上の状態文・EEW 札の枠が、赤い大きなパネル「<b>津波警報が発表されています</b> / 詳細については、ここをクリックしてください」+ 凡例 (赤線=津波警報・黄線=津波注意報) に<b>置き換わる</b>。地図は<b>東北・北海道へ寄って</b>(関東は端に残る)、<b>海岸線の太い線</b>が警報=赤・注意報=黄 (注意報の範囲が広く、警報は北海道太平洋中部と岩手)。黄色い矩形の格子が地図上に重なる (意味は未確認)", "左ペインの左上 + 地図", "津波予報の帯 + 沿岸線の色 + 凡例の津波の等級 + 地図のカメラ (津波予報区全体へ寄る)", "<b>うちの津波の見せ方と同じ構成</b>: 沿岸線の線の色と、凡例 (警報=赤・注意報=黄)、津波予報区全体へ寄るカメラ (うちは <code>tsunamiBox</code> で寄る)。同じ「左上の枠」を、状態文 → EEW の札 → 津波のパネルと<b>使い回す</b>。パネルが「クリックで詳細」というリンクを持つ点は、うちには無い (帯は表示のみ)", "確"],
    ["[同] 右の寄り図ペイン: 「<b>最大震度 5強</b> / 三陸沖 深さ10km <b>M7.4</b>」に更新 (震源・規模が確定)、日時の隣の欄が「<b>津波に関する情報を発表中</b>」(それまでは「津波の心配はありません」)。寄り図の最大震度は、震度速報を受けた時点 (16:55:41 には既に「5」) で更新される。寄り図は東北〜北海道の観測点の丸 (震度別の色)。右下は履歴", "右列", "詳細パネルの「津波」の行 (<code>TSUNAMI_TEXT</code>)", "文言は同じ種類 (「津波に関する情報を発表中」= うちの「津波警報等 発表中」)。津波予報が届くと、右の詳細の津波の欄が更新される点も同じ。M が EEW の M7.5 と確定の M7.4 で違う値を出している (値の出どころ・更新が別)", "確"],
    ["[2024/01/01 16:18:42 能登半島地震・津波警報発表中・EEW 予報 最終報 (PLUM 法)] <b>日本海側の海岸線</b>が、<b>津波警報=赤 (石川能登〜新潟〜富山の沿岸)</b>・<b>津波注意報=黄 (北海道〜東北の日本海側、山口までの西日本の沿岸、離島も)</b> で縁取られる。左の枠は縦に積む: EEW の札 (橙・予報・最終報) → 赤い札「津波警報発表中」→「強震最大震度 5−」「最大加速度 32.4 gal」。EEW の札は「<b>検知</b>」(発生でなく) と「<b>PLUM法による仮定震源要素</b>」", "全国図の沿岸 + 左の枠", "沿岸線の等級の色 + 津波の帯 + 凡例", "<b>日本海側でも同じ作り</b>: 沿岸線の色 (うちと同じ警報=赤・注意報=黄) + 小さな「津波警報発表中」の札。左の枠は、<b>状態文・EEW の札・津波の札・強震最大震度/最大加速度の枠を縦に積む</b> (共有スロットは 1 つの要素でなく、積み上がる列)。EEW が <b>PLUM 法</b>のとき、札の文言が「発生」→「検知」・「仮定震源要素」に変わる。うちの EEW が PLUM 法かを区別して持っているか: データ側に手がかりはある (P2P の <code>kind_code</code> が「19: 到達予想なし (PLUM 法など)」、Wolfx の <code>Arrive</code> が「主要動到達時刻の予測なし（PLUM 法による予測）」) が、画面側で区別しているかは未確認。<b>(この画像は保存されておらず、第三者の検証はできていない。私が画像を直接見て読み取った内容)</b>", "中"],
    ["[2024/01/01 16:23:17 能登半島地震・<b>大津波警報</b>発表中・EEW 警報 最終報] 左の枠の札が「<b>大津波警報発表中</b>」(<b>紫</b>)。海岸線は<b>大津波警報=紫 (能登半島の外周)・津波警報=赤 (日本海側のほぼ全域)・津波注意報=黄 (北海道〜東北の日本海側・山陰の一部)</b> の 3 色。EEW の札は警報 (赤) 6−・M6.3・深さ10km。地図は予想震度の背景の塗り (赤 6−・黄 4・緑 3・青 2) と観測点の円", "左の枠 + 全国図の沿岸", "津波の帯 (大津波警報=紫) + 沿岸線の色 (うちは大津波警報だけ線を太く 7px) + 凡例", "<b>津波の 3 等級が、うちの配色と同じ</b>: 注意報=黄・警報=赤・大津波警報=紫。札の色も等級で変わる (「津波警報発表中」=赤 → 「大津波警報発表中」=紫)。うちの帯も等級で色が変わる (<code>.tsunami-banner[data-grade]</code>)。違いは、うちが線の太さで大津波警報を強調するのに対し、JQuake は紫の縁取りで半島を囲む点。<b>(この画像は保存されておらず、第三者の検証はできていない。私が画像を直接見て読み取った内容)</b>", "中"],
    ["[2024/01/01 16:11:46 能登半島地震・EEW が <b>3 件同時</b>] 左上の EEW の札に「<b>2/3</b>」(3 件のうち 2 件目) が付き、第35報・予想最大震度 7・M7.4・深さ10km。ほかの EEW の震央にも ✕ が出る (能登と静岡沖の 2 か所を確認)。札はおそらく 3 件を順に巡回する", "地図の左上", "EEW の帯 (最大 3 件を並べて「ほか N 件」) + 震央の番号 (1・2…) + 画面外の矢印", "<b>複数 EEW の出し分けが違う</b>。うちは帯に最大 3 行を並べ、震央に番号を振る。JQuake は札 1 つで、「N/M」の巡回表示にして場所を節約する (巡回の間隔・順序は未確認)", "中"],
    ["[同] 左の凡例が「震度 / <b>予想 (背景)</b> / <b>計測 (円)</b>」の 2 系統。地図には、予想震度の<b>背景の塗り</b>と、観測点の<b>円 (数字入り)</b> が重なる。「強震最大震度」の枠に<b>観測の最大 7 (紫)</b>、「最大加速度」に <b>113 gal</b>", "地図の左", "予測の塗り (点滅・破線) と観測の塗り (うちは区別して描く)", "<b>予想と計測を、塗りと円で描き分ける</b>考え方はうちと同じ (うちは予測=破線・点滅、観測=実線)。観測の円・最大値は観測点のリアルタイムデータが要る", "確"],
    ["[同] 地図の右下に県別の「強震モニタ震度」の一覧 (石川県 7・新潟県 6−・富山県 6−・岐阜県 5+ …)、その下に「付近観測点データ」(東京都 東雲: 震度 1.2・加速度 1.47、大きな「1」)", "地図の右下", "なし (詳細の震度別一覧は確定情報の観測点)", "観測点のリアルタイム値の県別まとめ。データ源は同上", "要"],
    ["[同] 右の列: 上に「最大震度 6+ / 震源調査中 / 2024/01/01 16:10」(震度速報の見出し、<b>地図なし</b>)、履歴 (大きな行 × 7、行の頭に震度の色札)、その下に「<b>現在地の予想震度と到達までの時間</b>」(3 地点分: 予想震度の札 + 絵 + 「到達まで 0:00」)", "右列", "カウントダウン (地図の左上に出す 1 件)", "<b>うちの「主要動の到達カウントダウン」と同じ部品</b>。違いは、<b>登録した複数の地点 (3 つ) を同時に</b>出す点と、予想震度に<b>震度の体感を示す絵</b>を添える点。うちは自分の地点 1 つだけで、地図の上に重ねる。また、確定した震源が無い間は寄り図を出さず、見出しだけになる", "確"],
    ["右列の上: 最新地震へ寄った地図 + 重ねた詳細 (日時・津波・最大震度・震源・深さ・M)", "右カラムの上", "地図 (全体で 1 枚がカメラで寄る) + 詳細パネル (右パネルの文字)", "<b>地図が 2 枚 (全国 + 寄り) 同時</b>。うちは 1 枚の地図が寄ったり戻ったりする。詳細は文字のパネルでなく、地図の上の重ね物", "確"],
    ["寄った地図の凡例 (① 震度 / ✕ 震央) と出典", "寄った地図の右下・左下", "凡例 (地図の左下) / 出典 (右パネルの下)", "凡例も出典も、その地図の上に載る (地図ごとに持つ)", "中"],
    ["履歴: 大きな行 6 件 (日時 + M + 震源名 + 大きな最大震度)", "右列の下", "履歴の一覧 (右パネル、最大 100 件、細い行)", "同じ部品で行の見た目を「大きな行」にできるか。件数は少なく固定。<b>寄り図の地震は一覧に入らない</b> (寄り図 = 最新、一覧 = それを除いた残り)。重複を出さない規則が、地図と履歴の部品設計に直接効く", "中"],
  ],
  takeaways: [
    "JQuake は「<b>うちの 2 カラムに近い</b>」ため、desktop プロファイルの発展形として参照しやすい。差は主に右列: 文字の詳細パネルを、<b>寄った小さな地図 + 重ねた詳細</b>に置き換える点。",
    "<b>JQuake の EEW 中は、左の全国図だけが反応</b>する (寄る・塗る・点が色づく)。右上の寄り地図は直近に確定した地震を固定で見せ続ける。したがって 2 枚目の地図は「カメラ追従なしの固定ビュー + 重ね物」で足り、1 枚目の地図 (うちの JapanMap に近い) が EEW 担当になる。2 枚目を作る負担は思ったより小さいかもしれない (要確認)。",
    "<b>状態文の枠と EEW の札は同じ領域</b>を使う (「発表されていません」→ EEW の札に入れ替わる)。「常設の領域 + 中身が変わる」流儀は JQuake にも共通 (JDQ と同じ)。",
    "<b>津波の見せ方は参照画面で割れている</b>: JDQ は「到達予想」の表 (種別・区域・時刻・高さ) を地図の上に重ね、JQuake は「津波警報発表中」の小さな札 + 海岸線の色だけ。うちは帯 (区域名) + 沿岸線の色 + 詳細 (高さ・直ちに来襲) で、情報量は JDQ に近く、見せ方は JQuake に近い。どちらの方向に寄せるかは、部品の「情報量の段階 (簡易/詳細)」を持たせる設計の論点になる。",
    "どちらの参照 (JDQ・JQuake) でも<b>地図が 2 枚同時</b>に出る。「地図を複数持てる」ことが、参照を取り入れるうえで共通の前提になる (第 11.5b の「地図が単一インスタンス」)。",
    "どちらも<b>全国図に観測点の点群 (計測震度)</b> を出す。レイアウトより先に、そのデータ源の可否を調べる必要がある (現行の P2P地震情報・Wolfx には無い)。",
    "詳細・凡例・出典を「地図ごとの重ね物」にする、というのが共通の考え方。部品が「どの地図に属するか」を持てると、地図が 2 枚あっても整理できる。",
  ],
};

// 次に確認したい場面 (参照画面の探索の依頼リスト)。優先順
window.WANTED = [
  { n: 1, t: "確定した地震の表示 (JDQ)", d: "<b>確認できた</b> (震度速報 → 震源確定・観測点の点・下端の観測点名の列挙)。残りは、複数の地震が連続したときと、津波予報つきの確定情報。" },
  { n: 2, t: "警報 (赤) の EEW (JDQ)", d: "予想最大震度 5弱以上の警報のときの札の色・点滅・音と連動する表示。うちは警報=赤・予報=橙。JQuake は同じ配色だった。" },
  { n: 3, t: "津波予報の発表中 (JDQ・JQuake)", d: "<b>確認済み</b>: JDQ の発表直後 (全国ペインの「津波到達予想」パネル)、JQuake の発表直後と約 4 分後 (札 + 沿岸線の色)、JQuake の日本海側の津波警報と大津波警報 (能登半島地震。3 等級の色)。<b>未確認</b>: JDQ の沿岸線の色づけ・注意報のみの時間帯・解除 (取得できたアーカイブが 16:59 頃までのため。今後の宿題)。" },
  { n: 4, t: "揺れが収まって平時に戻るところ (JDQ・JQuake)", d: "<b>おおむね確認できた</b>: EEW の最終報から 3 分 20 秒以内に、全国ペインが状態文へ戻り、履歴が重なる。右ペインは最新の確定地震のまま残る。残りは、<b>下端の帯がいつ平時の案内に戻るか</b>と、右ペインが「最近の地震」に戻る時刻。EEW・地震情報のあと、いつ・どう元の画面に戻るか。うちは「落ち着く (settle)」と時間で戻す。参照画面の戻し方と時間を比べたい。" },
  { n: 5, t: "複数の地震が同時のとき (JDQ・JQuake)", d: "<b>JQuake の EEW 3 件同時は確認できた</b> (「2/3」の巡回表示)。JDQ の複数同時と、確定した地震が重なるときは未確認。元の依頼: EEW が 2 件同時、または EEW と確定した地震が重なったときの出し分け (札・地図)。うちは優先度で 1 件を主役にし、ほかは番号と矢印で示す。" },
  { n: 6, t: "気象警報パネルの別の場面 (JDQ)", d: "警報・特別警報・多数の区域があるとき (段階チップの使い方、区域の数が多いときの省略)。うちは帯 (警報以上のみ) + 地図の塗り。" },
  { n: 7, t: "遠地・離島の地震 (JDQ・JQuake)", d: "沖縄・小笠原・遠い海外の地震のとき、別枠や画面外の扱い。うちは別枠 + 画面外の矢印。" },
  { n: 8, t: "接続断・時刻同期の異常 (JDQ)", d: "「NTP server」の表示が異常のとき、データが来ないときの見せ方。うちは時計の枠の色 + 「切断中」。" },
];
