// 配信用の表示 (?broadcast=1)。eq-server broadcast が画面の無い Chrome で開き、そのまま配信する。
// 操作ボタンを隠し、警戒音と BGM を最初から鳴らす (配信用の Chrome は毎回まっさらなので、ここで設定を書き込む)。
// &sink=<出力先の名前> を付けると、音 (警戒音・BGM) をその出力先 (Mac なら BlackHole 2ch) へ流す。Mac 全体の出力先は変えない。
// ほかのモジュールが設定を読む前に動くよう、main.ts で最初に読み込む。

const params = new URLSearchParams(location.search);
export const broadcasting = params.has("broadcast");
const sinkName = broadcasting ? params.get("sink") : null;

interface SinkTarget {
  setSinkId(id: string): Promise<void>;
}

/**
 * 音の出力先を &sink= の名前の機器にする。画面の無い Chrome は、マイクを一度開くまで機器の名前を見せないので、
 * 名前が見えなければマイクを開いてすぐ閉じる (音は使わない。確認は eq-server broadcast が自動で許可する)
 */
export async function routeAudio(target: SinkTarget): Promise<void> {
  if (!sinkName) return;
  for (let i = 0; i < 15; i++) {
    let devices = await navigator.mediaDevices.enumerateDevices().catch(() => []);
    if (devices.every((d) => !d.label)) {
      const s = await navigator.mediaDevices.getUserMedia({ audio: true }).catch(() => null);
      s?.getTracks().forEach((t) => t.stop());
      devices = await navigator.mediaDevices.enumerateDevices().catch(() => []);
    }
    const d = devices.find((x) => x.kind === "audiooutput" && x.label.includes(sinkName));
    if (d) {
      await target.setSinkId(d.deviceId).then(
        () => console.info(`audio output: ${d.label}`),
        (e) => console.warn("setSinkId", String(e)),
      );
      return;
    }
    await new Promise((r) => setTimeout(r, 2000));
  }
  console.warn(`audio output "${sinkName}" not found`);
}

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
