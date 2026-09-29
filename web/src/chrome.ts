// 画面のまわり: 日時表示・接続状態・テロップ・音のボタン。

import { clockParts } from "./clock.ts";
import type { Status } from "./connection.ts";
import { onSoundStateChange, play, setSoundEnabled, soundEnabled, soundReady, unlock } from "./sound.ts";
import { $, tapWord } from "./dom.ts";

/** テロップの文を切り替える間隔 */
const TELOP_INTERVAL_MS = 8000;

let telopMessages: string[] = [];

/** テロップ。地震の情報を出している間は邪魔をしないよう消す */
export function renderTelop(now: number, busy: boolean): void {
  const el = $("#telop");
  el.hidden = busy || telopMessages.length === 0;
  if (el.hidden) return;
  const slot = Math.floor(now / TELOP_INTERVAL_MS);
  const text = telopMessages[slot % telopMessages.length];
  if (el.textContent === text) return;
  // 切り替え時は一度消してから出す
  el.classList.add("fading");
  setTimeout(() => {
    el.textContent = text;
    el.classList.remove("fading");
  }, 400);
}

export function loadTelop(): void {
  fetch("api/telop")
    .then((r) => (r.ok ? r.json() : []))
    .then((m: unknown) => {
      if (Array.isArray(m)) telopMessages = m.filter((x): x is string => typeof x === "string");
    })
    .catch(() => {});
}

export function renderClock(now: number): void {
  const c = clockParts(now);
  const set = (id: string, text: string) => {
    const el = $(id);
    if (el.textContent !== text) el.textContent = text;
  };
  set("#c-year", `${c.year}年`);
  set("#c-month", `${c.month}月`);
  set("#c-day", `${c.day}日`);
  set("#c-wd", `(${c.weekday})`);
  set("#c-hm", c.hm);
  if ($("#c-sec").textContent !== c.sec) {
    set("#c-sec", c.sec);
    // 秒が変わるたびに拍動させる (アニメーションを最初からやり直す)
    const beat = $("#c-beat");
    beat.classList.remove("beat");
    void beat.offsetWidth;
    beat.classList.add("beat");
  }
}

export function setStatus(s: Status): void {
  $("#clock").dataset.status = s;
  $("#c-status").textContent = { connecting: "接続中", open: "時刻同期", closed: "切断中" }[s];
}

export function renderSound(): void {
  const waiting = soundEnabled() && !soundReady();
  // 設定は ON だがブラウザの制限でまだ鳴らせないときは、地図の上に案内を出す
  $("#sound-hint").hidden = !waiting;
  const btn = $("#sound");
  const text = !soundEnabled() ? "音 OFF" : waiting ? `音 ON (${tapWord}で有効化)` : "音 ON";
  if (btn.textContent === text) return;
  btn.textContent = text;
  btn.classList.toggle("active", soundEnabled() && !waiting);
  btn.classList.toggle("waiting", waiting);
}

function enableSound(): void {
  unlock();
  play("low"); // 確認用
}

$("#sound").addEventListener("click", () => {
  // 「タップ (クリック) で有効化」の状態で押したら、OFF にせず有効化する
  if (soundEnabled() && !soundReady()) enableSound();
  else {
    setSoundEnabled(!soundEnabled());
    if (soundEnabled()) enableSound();
  }
  renderSound();
});

$("#sound-hint").textContent = `${tapWord}すると警戒音が鳴るようになります`;
$("#sound-hint").addEventListener("click", enableSound);

onSoundStateChange(renderSound);

if (soundEnabled()) {
  // サイトの設定で音声を許可していれば、操作なしでこのまま鳴らせるようになる
  unlock();
  // だめなら最初の操作で有効化する
  document.addEventListener("pointerdown", unlock, { once: true });
  document.addEventListener("keydown", unlock, { once: true });
}
