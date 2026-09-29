// 平時の BGM。サーバに置いた曲をファイル名順に流し、曲の変わり目はクロスフェードでつなぐ。
// 地震の表示の間とデモ中は直ちに一時停止し (警戒音を優先)、平時に戻れば続きから少しずつ音を上げて流す。

import { $, tapWord } from "./dom.ts";
import { saveSettings } from "./personal.ts";
import { nextTrack } from "./playlist.ts";
import { app } from "./state.ts";

interface Track {
  file: string;
  title: string;
  artist: string | null;
}

/** 曲の変わり目で重ねる秒数 */
const CROSSFADE_SEC = 3;
/** 平時に戻って再開するときに音を上げる秒数 */
const RESUME_FADE_SEC = 1;

/** 交互に使う 2 つのプレイヤー (次の曲を重ねて流すため) */
const players = [new Audio(), new Audio()];
const playing: (Track | null)[] = [null, null];
let cur = 0;
let tracks: Track[] = [];
/** ブラウザに再生を止められた (利用者が一度操作するまで流せない) */
let blocked = false;
/** 再生を始めている途中 (play() の結果待ち) */
let starting = false;
/** 平時か (流してよいか) */
let allowed = false;
/** 音量を変えている最中のタイマー */
let fadeTimer = 0;

const level = () => app.settings.bgmVolume / 100;

/** 曲の一覧を取り直す (サーバは毎回ディレクトリを読み直すので、差し替えた曲もここで入る) */
export async function loadTracks(): Promise<void> {
  const res = await fetch("api/bgm");
  tracks = res.ok ? await res.json() : [];
  renderBgm();
}

/** 音量を sec 秒かけて変える。steps は [プレイヤー, 始め, 終わり]。終わったら done */
function fade(steps: [HTMLAudioElement, number, number][], sec: number, done?: () => void): void {
  clearInterval(fadeTimer);
  const start = performance.now();
  const tick = () => {
    const k = Math.min(1, (performance.now() - start) / (sec * 1000));
    for (const [p, from, to] of steps) p.volume = Math.max(0, Math.min(1, from + (to - from) * k));
    if (k >= 1) {
      clearInterval(fadeTimer);
      done?.();
    }
  };
  tick();
  fadeTimer = window.setInterval(tick, 50);
}

/** 次の曲へ。crossfade なら今の曲に重ねて入れ替える */
async function playNext(crossfade: boolean): Promise<void> {
  if (starting) return;
  starting = true;
  await loadTracks().catch(() => {});
  const file = nextTrack(
    tracks.map((t) => t.file),
    playing[cur]?.file ?? null,
  );
  const track = tracks.find((t) => t.file === file) ?? null;
  const old = players[cur];
  const next = 1 - cur;
  const p = players[next];
  if (!track || !allowed || !app.settings.bgm) {
    starting = false;
    return renderBgm();
  }
  p.src = `bgm/${encodeURIComponent(track.file)}`;
  p.volume = crossfade ? 0 : level();
  try {
    await p.play();
    blocked = false;
    playing[next] = track;
    cur = next;
    if (crossfade && !old.paused) {
      fade([
        [p, 0, level()],
        [old, old.volume, 0],
      ], CROSSFADE_SEC, () => old.pause());
    } else {
      old.pause();
    }
  } catch {
    // 自動再生の制限 (NotAllowedError) など。利用者の操作を待つ
    blocked = true;
  }
  starting = false;
  renderBgm();
}

/** 止めていた曲を続きから流す (音を少しずつ上げる) */
async function resume(): Promise<void> {
  const p = players[cur];
  if (starting || !playing[cur]) return;
  starting = true;
  p.volume = 0;
  try {
    await p.play();
    blocked = false;
    fade([[p, 0, level()]], RESUME_FADE_SEC);
  } catch {
    blocked = true;
  }
  starting = false;
  renderBgm();
}

function stopAll(): void {
  clearInterval(fadeTimer);
  for (const p of players) if (!p.paused) p.pause();
}

/** 画面の更新 (tick) ごと: 平時なら流し、そうでなければ直ちに止める */
export function updateBgm(quiet: boolean): void {
  allowed = quiet;
  const want = app.settings.bgm && quiet && tracks.length > 0;
  if (!want) stopAll();
  else if (!playing[cur]) void playNext(false);
  else if (players[cur].paused && !blocked) void resume();
  renderBgm();
}

function renderBgm(): void {
  const btn = $("#bgm");
  btn.hidden = tracks.length === 0;
  const text = !app.settings.bgm ? "BGM OFF" : blocked ? `BGM ON (${tapWord}で開始)` : "BGM ON";
  if (btn.textContent !== text) btn.textContent = text;
  btn.classList.toggle("active", app.settings.bgm && !blocked);
  btn.classList.toggle("waiting", app.settings.bgm && blocked);
  const t = playing[cur];
  const on = t != null && !players[cur].paused;
  const name = t ? `${t.title}${t.artist ? ` / ${t.artist}` : ""}` : "";
  const label = $("#bgm-now");
  const now = on ? `BGM: ${name}` : "";
  if (label.textContent !== now) label.textContent = now;
  label.hidden = !on;
  btn.title = on ? `再生中: ${name}` : "平時に BGM を流します (地震の表示の間は止まります)";
}

for (const [i, p] of players.entries()) {
  p.preload = "auto";
  // 終わりが近づいたら次の曲を重ねる (短すぎる曲は重ねずに終わってから次へ)
  p.addEventListener("timeupdate", () => {
    if (i !== cur || starting || !Number.isFinite(p.duration) || p.duration < CROSSFADE_SEC * 3) return;
    if (p.duration - p.currentTime <= CROSSFADE_SEC) void playNext(true);
  });
  p.addEventListener("ended", () => {
    if (i === cur) void playNext(false);
  });
  // 読めない曲は少し待って次へ (全部読めないときに詰めて繰り返さないように)
  p.addEventListener("error", () => {
    if (i === cur) window.setTimeout(() => void playNext(false), 3000);
  });
}

// ボタンは利用者の操作なので、その場で再生を始める (自動再生の制限を解く)
$("#bgm").addEventListener("click", () => {
  const on = !app.settings.bgm || blocked;
  app.settings = { ...app.settings, bgm: on };
  saveSettings(app.settings);
  blocked = false;
  if (!on) stopAll();
  else if (playing[cur]) void resume();
  else void playNext(false);
  renderBgm();
});

const volume = $<HTMLInputElement>("#bgm-volume");
volume.value = String(app.settings.bgmVolume);
volume.addEventListener("input", () => {
  app.settings = { ...app.settings, bgmVolume: Number(volume.value) };
  saveSettings(app.settings);
  clearInterval(fadeTimer);
  players[cur].volume = level();
});

// 前回 BGM を ON にしていて自動再生を止められたときは、画面のどこかを操作したら流す
document.addEventListener("pointerdown", (e) => {
  // BGM ボタン自体の操作はボタンの処理に任せる
  if ((e.target as Element).closest("#bgm")) return;
  if (blocked && app.settings.bgm) {
    blocked = false;
    if (playing[cur]) void resume();
    else void playNext(false);
  }
});
