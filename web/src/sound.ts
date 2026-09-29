// 警戒音 (WebAudio で合成。音声ファイルは持たない)。
// ブラウザは利用者の操作があるまで音を出せないことが多い。ページを開いた直後にも unlock() を試し
// (サイトの設定で音声を許可していれば操作なしで鳴らせる)、だめならボタンやタップで unlock() する。

import type { AlertLevel } from "./alert.ts";

const KEY = "eq-sound";
let ctx: AudioContext | null = null;

export function soundEnabled(): boolean {
  try {
    return localStorage.getItem(KEY) === "on";
  } catch {
    return false;
  }
}

export function setSoundEnabled(on: boolean): void {
  try {
    localStorage.setItem(KEY, on ? "on" : "off");
  } catch {
    // 保存できなくてもこのページの間は有効
  }
}

let onChange: () => void = () => {};

/** 鳴らせる状態が変わったとき (有効化できた、など) に呼ぶ */
export function onSoundStateChange(f: () => void): void {
  onChange = f;
}

/** 鳴らせる状態にする。操作の中で呼ぶのが確実だが、許可されていれば操作なしでも通る */
export function unlock(): void {
  if (!ctx) {
    ctx = new AudioContext();
    ctx.onstatechange = () => onChange();
  }
  ctx.resume().then(
    () => onChange(),
    () => {},
  );
}

export function soundReady(): boolean {
  return ctx?.state === "running";
}

function tone(c: AudioContext, freq: number, at: number, dur: number, type: OscillatorType, peak: number): void {
  const t = c.currentTime + at;
  const osc = c.createOscillator();
  const gain = c.createGain();
  osc.type = type;
  osc.frequency.value = freq;
  gain.gain.setValueAtTime(0.0001, t);
  gain.gain.linearRampToValueAtTime(peak, t + 0.01);
  gain.gain.exponentialRampToValueAtTime(0.0001, t + dur);
  osc.connect(gain).connect(c.destination);
  osc.start(t);
  osc.stop(t + dur + 0.05);
}

export function play(level: AlertLevel | "pip" | "feel"): void {
  if (!soundEnabled() || !ctx || ctx.state !== "running") return;
  const c = ctx;
  switch (level) {
    case "low": // ピンポン 1 回
      tone(c, 880, 0, 0.5, "sine", 0.25);
      tone(c, 660, 0.25, 0.7, "sine", 0.25);
      break;
    case "medium": // チャイム 2 回
      for (const at of [0, 0.9]) {
        tone(c, 988, at, 0.5, "sine", 0.3);
        tone(c, 784, at + 0.3, 0.8, "sine", 0.3);
      }
      break;
    case "info": // 上がる 3 音 (確定情報の案内)
      [660, 880, 1100].forEach((f, i) => tone(c, f, i * 0.18, 0.6, "sine", 0.22));
      break;
    case "pip": // 波の広がり中の刻み
      tone(c, 1200, 0, 0.12, "sine", 0.15);
      break;
    case "feel": // 地震感知情報 (利用者の「揺れた」報告): 低めの控えめな 2 音
      tone(c, 523, 0, 0.35, "triangle", 0.14);
      tone(c, 523, 0.3, 0.35, "triangle", 0.14);
      break;
    case "strong": // 2 音を交互に繰り返す
      for (let i = 0; i < 12; i++) tone(c, i % 2 ? 770 : 960, i * 0.2, 0.18, "square", 0.12);
      break;
  }
}
