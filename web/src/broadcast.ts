// 配信用の表示 (?broadcast=1)。eq-server broadcast が画面の無い Chrome で開き、そのまま配信する。
// 操作ボタンを隠し、警戒音と BGM を最初から鳴らす (配信用の Chrome は毎回まっさらなので、ここで設定を書き込む)。
// ほかのモジュールが設定を読む前に動くよう、main.ts で最初に読み込む。

export const broadcasting = new URLSearchParams(location.search).has("broadcast");

if (broadcasting) {
  try {
    localStorage.setItem("eq-sound", "on");
    const s = JSON.parse(localStorage.getItem("eq-settings") ?? "{}");
    localStorage.setItem("eq-settings", JSON.stringify({ ...s, bgm: true }));
  } catch {
    // 保存できなければ音は鳴らないが、表示はできる
  }
  document.documentElement.classList.add("broadcast");
}
