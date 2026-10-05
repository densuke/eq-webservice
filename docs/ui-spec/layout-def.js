// 画面の構成を「領域の入れ子 + 大きさ + 部品の名前」で書く定義 (案)。layout-preview.html がこれをそのまま描く。
//
// 書式
//   ノード = 容器 { dir: "row" | "column", size, children: [ノード…], overlays?, scroll? }
//          | 部品 { slot: 部品の名前, size, overlays? }
//   size   = "fill"          残りを分ける (flex 1)。"fill:2" は重み 2
//          | "70%"           親の向きに対する割合
//          | "380px"         固定
//          | "auto"          部品の中身の大きさ (COMPONENTS の size)
//          | "60vh"          画面の高さに対する割合 (ページ全体がスクロールする compact 用)
//   overlays = { 隅: 重ね方 }  隅 = "top-left" | "top-right" | "bottom-left" | "bottom-right" | "top" | "bottom"
//   重ね方    = { flow: "column" | "row", items: [部品の名前 | 重ね方…] }  上から下 (左から右) の順に積む。隅との余白と間隔は 10px
//             (部品どうしの位置を px で手計算しない: いまの CSS の left:246px・top:242px などが不要になる)
//
// 部品は COMPONENTS に登録した名前で書く。dom は現行の web の要素 (移行のときの対応)。

window.LD_COMPONENTS = {
  // 画面の主役
  main: { label: "メインの地図", dom: "#map", kind: "map" },
  sub: { label: "サブの地図 (最新の地震へ寄った図)", dom: null, kind: "map" },
  // 右パネル系
  detail: { label: "地震の詳細", dom: "#detail", kind: "panel", size: { h: 74 } },
  history: { label: "地震の履歴", dom: "#list", kind: "panel", size: { h: 120 } },
  notice: { label: "お知らせ", dom: "#banner", kind: "panel", size: { h: 120 } },
  credit: { label: "出典", dom: "footer.credit", kind: "panel", size: { h: 100 } },
  settings: { label: "設定 / デモのパネル", dom: "#settings-panel, #demo-panel", kind: "panel", size: { h: 0 } },
  // バー・帯
  topbar: { label: "上部バー", dom: "header.topbar", kind: "bar", size: { h: 37 } },
  banners: { label: "帯 (EEW・津波・気象警報)", dom: "#eew-banner, #tsunami-banner, #warn-banner", kind: "bar", size: { h: 31 } },
  ticker: { label: "下端の帯 (状態ごとの文)", dom: "#telop", kind: "bar", size: { h: 40 } },
  // 地図の上の重ね物
  status: { label: "状態枠 (状態文 / EEW の札 / 津波)", dom: null, kind: "overlay", size: { w: 420, h: 150 } },
  inset: { label: "別枠 (南西諸島)", dom: ".inset-okinawa", kind: "overlay", size: { w: 226, h: 220 } },
  caption: { label: "天気の札の案内", dom: "#weather-caption", kind: "overlay", size: { w: 143, h: 26 } },
  countdown: { label: "到達カウントダウン", dom: "#countdown", kind: "overlay", size: { w: 260, h: 70 } },
  legend: { label: "凡例", dom: ".legend", kind: "overlay", size: { w: 54, h: 238 } },
  clock: { label: "時計", dom: "#clock", kind: "overlay", size: { w: 181, h: 91 } },
  ogasawara: { label: "別枠 (小笠原)", dom: ".inset-ogasawara", kind: "overlay", size: { w: 52, h: 150 } },
  toast: { label: "通知 (トースト・音の案内)", dom: "#tour-toast, #sound-hint", kind: "overlay", size: { w: 240, h: 34 } },
  gauge: { label: "計測震度ゲージ", dom: null, kind: "overlay", size: { w: 40, h: 320 } },
  near: { label: "付近観測点データ", dom: null, kind: "overlay", size: { w: 260, h: 110 } },
  camera: { label: "ライブカメラ (対象外)", dom: null, kind: "skip", size: { w: 420 } },
};

// プロファイル。when は選ぶ条件 (幅・高さ。将来はコンテナクエリ)。target: "broadcast" は配信用で、画面の大きさでは選ばない。sizes で部品の大きさをプロファイルごとに上書きできる
window.LD_LAYOUTS = [
  {
    name: "landscape",
    note: "【案】横向きスマホ (幅 801 以上・高さ 480 以下)。いまは regular に落ちて別枠が凡例を隠す (844×390 で実測)。別枠・凡例を小さくし、お知らせ・出典を出さず、右パネルを細く",
    when: { minWidth: 801, maxHeight: 480 },
    preview: [844, 390],
    sizes: { inset: { w: 120, h: 117 }, caption: { w: 0 }, legend: { w: 40, h: 150 }, clock: { w: 150, h: 75 }, notice: { h: 0 }, credit: { h: 0 }, ogasawara: { w: 0 } },
    root: {
      dir: "column",
      children: [
        { slot: "topbar", size: "auto" },
        { slot: "banners", size: "auto" },
        {
          dir: "row", size: "fill",
          children: [
            {
              slot: "main", size: "fill",
              overlays: {
                "top-left": { flow: "row", items: ["inset", "legend"] },
                "top-right": { flow: "column", items: ["countdown", "toast"] },
                "bottom-right": { flow: "column", items: ["clock"] },
              },
            },
            { dir: "column", size: "300px", children: [{ slot: "settings", size: "auto" }, { slot: "detail", size: "auto" }, { slot: "history", size: "fill" }] },
          ],
        },
      ],
    },
  },
  {
    name: "regular",
    note: "いまの PC の配置 (地図 + 右パネル 380px)",
    when: { minWidth: 801 },
    preview: [1280, 720],
    root: {
      dir: "column",
      children: [
        { slot: "topbar", size: "auto" },
        { slot: "banners", size: "auto" },
        {
          dir: "row", size: "fill",
          children: [
            {
              slot: "main", size: "fill",
              overlays: {
                "top-left": { flow: "column", items: [{ flow: "row", items: ["inset", "caption"] }, "countdown"] },
                "bottom-left": { flow: "column", items: ["legend"] },
                "bottom-right": { flow: "column", items: ["ogasawara", "clock"] },
                bottom: { flow: "column", items: ["toast"] },
              },
            },
            {
              dir: "column", size: "380px",
              children: [
                { slot: "settings", size: "auto" },
                { slot: "detail", size: "auto" },
                { slot: "history", size: "fill" },
                { slot: "notice", size: "fill" },
                { slot: "credit", size: "auto" },
              ],
            },
          ],
        },
      ],
    },
  },
  {
    name: "compact",
    note: "いまのスマホの配置 (縦に積み、ページ全体がスクロール)",
    when: { maxWidth: 800 },
    preview: [390, 844],
    scroll: "page",
    sizes: { topbar: { h: 91 }, inset: { w: 144, h: 140 }, clock: { w: 150, h: 75 }, legend: { w: 50, h: 198 }, caption: { w: 129, h: 23 } },
    root: {
      dir: "column",
      children: [
        { slot: "topbar", size: "auto" },
        { slot: "banners", size: "auto" },
        {
          slot: "main", size: "60vh",
          overlays: {
            "top-left": { flow: "column", items: [{ flow: "row", items: ["inset", "caption"] }, "legend"] },
            // いまの CSS は countdown を bottom:64px の中央に置く。時計 (bottom:6px・高さ 75) と横幅が重なると縦にも重なりうる (要確認)。
            // 定義では右下に縦に積んで、重ならないようにする
            "bottom-right": { flow: "column", items: ["toast", "countdown", "clock"] },
          },
        },
        { slot: "settings", size: "auto" },
        { slot: "detail", size: "auto" },
        { slot: "history", size: "auto" },
        { slot: "notice", size: "auto" },
        { slot: "credit", size: "auto" },
      ],
    },
  },
  {
    name: "broadcast-native",
    note: "いまの配信 (Rust で描く 1280×720 固定。帯は地図の上に重ねる)",
    when: { target: "broadcast" },
    preview: [1280, 720],
    sizes: { topbar: { h: 36 }, legend: { w: 34, h: 147 }, clock: { w: 176, h: 74 } },
    root: {
      dir: "column",
      children: [
        { slot: "topbar", size: "auto" },
        {
          dir: "row", size: "fill",
          overlays: { top: { flow: "column", items: ["banners"] } },
          children: [
            {
              slot: "main", size: "900px",
              overlays: {
                "top-left": { flow: "column", items: ["inset"] },
                "bottom-left": { flow: "column", items: ["legend"] },
                "bottom-right": { flow: "column", items: ["clock"] },
              },
            },
            {
              dir: "column", size: "fill",
              children: [
                { slot: "detail", size: "144px" },
                { slot: "history", size: "fill" },
                { slot: "notice", size: "104px" },
                { slot: "credit", size: "90px" },
              ],
            },
          ],
        },
      ],
    },
  },
  {
    name: "jquake-like",
    note: "参照: JQuake 型 (左 68% の全国図 + 右列に寄り図と大きな行の履歴)",
    when: { target: "broadcast" },
    preview: [1920, 1080],
    sizes: { status: { w: 600, h: 230 }, history: { h: 0 } },
    root: {
      dir: "row",
      children: [
        {
          dir: "column", size: "68%",
          children: [
            {
              slot: "main", size: "fill",
              overlays: {
                "top-left": { flow: "column", items: ["status", { flow: "row", items: ["gauge", "inset"] }] },
                "bottom-right": { flow: "column", items: ["near", "clock"] },
              },
            },
            { slot: "ticker", size: "auto" },
          ],
        },
        {
          dir: "column", size: "fill",
          children: [
            { slot: "sub", size: "41%" },
            { slot: "history", size: "fill" },
          ],
        },
      ],
    },
  },
  {
    name: "jdq-like",
    note: "参照: JDQ 型 (上段: 全国図 50% + 最近の地震 / 下段: 時計とカメラ + 情報パネル群)",
    when: { target: "broadcast" },
    preview: [1920, 1080],
    sizes: { status: { w: 420, h: 160 }, history: { h: 0 }, clock: { w: 0 } },
    root: {
      dir: "column",
      children: [
        {
          dir: "row", size: "74%",
          children: [
            {
              slot: "main", size: "50%",
              overlays: { "top-left": { flow: "column", items: ["status", { flow: "row", items: ["gauge", "inset"] }] } },
            },
            { slot: "sub", size: "fill" },
          ],
        },
        {
          dir: "row", size: "fill",
          children: [
            { dir: "row", size: "50%", children: [{ slot: "camera", size: "44%" }, { slot: "clock", size: "fill" }] },
            { dir: "column", size: "fill", children: [{ slot: "history", size: "fill" }, { slot: "ticker", size: "auto" }] },
          ],
        },
      ],
    },
  },
];
