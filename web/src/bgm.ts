// 平時の BGM。音楽は Icecast が配信し、この画面はその配信を鳴らすだけ。
// 地震の表示の間とデモ中は直ちに止め (警戒音を優先。配信の受信もやめる)、平時に戻ればそのときの放送から少しずつ音を上げて流す。

import { mixerAudio, notifyBgm, routeAudio } from "./broadcast.ts";
import { $, tapWord } from "./dom.ts";
import { saveSettings } from "./personal.ts";
import { app } from "./state.ts";

interface BgmConfig {
  /** 配信の URL (同じサイトの中) */
  stream: string;
  /** 再生中の曲名を取る Icecast の状態 (status-json.xsl)。空なら曲名は出さない */
  status: string;
}

/** 流し始めるときに音を上げる秒数 */
const FADE_IN_SEC = 1;
/** 曲名を取り直す間隔 */
const TITLE_MS = 15_000;
/** 配信が途切れたときにつなぎ直すまで */
const RETRY_MS = 5_000;

const audio = new Audio();
void routeAudio(audio);
let cfg: BgmConfig | null = null;
/** ブラウザに再生を止められた (利用者が一度操作するまで流せない) */
let blocked = false;
/** 流し始めている途中 (play() の結果待ち) */
let starting = false;
/** 平時か (流してよいか) */
let allowed = false;
let retryAt = 0;
let title = "";
let titleAt = 0;
let fadeTimer = 0;

const level = () => app.settings.bgmVolume / 100;
const wanted = () => cfg != null && app.settings.bgm && allowed;

export async function loadBgmConfig(): Promise<void> {
  const res = await fetch("api/bgm");
  cfg = res.ok ? await res.json() : null;
  renderBgm();
}

/** 配信を最初からつなぐ (受信を止めていたので、そのときの放送から) */
async function start(): Promise<void> {
  if (starting || !cfg || !wanted()) return;
  starting = true;
  clearInterval(fadeTimer);
  audio.src = `${cfg.stream}?t=${Date.now()}`;
  audio.volume = 0;
  try {
    await audio.play();
    blocked = false;
    const t0 = performance.now();
    fadeTimer = window.setInterval(() => {
      const k = Math.min(1, (performance.now() - t0) / (FADE_IN_SEC * 1000));
      audio.volume = level() * k;
      if (k >= 1) clearInterval(fadeTimer);
    }, 50);
    titleAt = 0;
  } catch (e) {
    if (e instanceof DOMException && e.name === "NotAllowedError") {
      // 自動再生の制限。利用者の操作を待つ
      blocked = true;
    } else {
      // 配信が途切れている (送り出しのつなぎ直し中など)。少し待ってつなぎ直す
      // (操作を待つ扱いにすると、誰も操作しない配信用の画面では BGM が止まったままになる)
      retryAt = Date.now() + RETRY_MS;
      stop();
    }
  }
  starting = false;
  renderBgm();
}

/** 止めて、配信の受信もやめる */
function stop(): void {
  clearInterval(fadeTimer);
  if (!audio.paused || audio.getAttribute("src")) {
    audio.pause();
    audio.removeAttribute("src");
    audio.load();
  }
}

/** 画面の更新 (tick) ごと: 平時なら流し、そうでなければ直ちに止める */
export function updateBgm(quiet: boolean): void {
  allowed = quiet;
  // 配信の mixer 音声: 流す・止めるを eq-server に知らせるだけ (音はページで鳴らさない)
  if (mixerAudio) return notifyBgm(wanted(), app.settings.bgmVolume);
  if (!wanted()) stop();
  else if (audio.paused && !blocked && Date.now() >= retryAt) void start();
  if (!audio.paused && cfg?.status && Date.now() - titleAt > TITLE_MS) void loadTitle();
  renderBgm();
}

/** 再生中の曲名 (Icecast の状態。日本語は &#12486; のような文字参照で来るので戻す) */
async function loadTitle(): Promise<void> {
  titleAt = Date.now();
  try {
    const res = await fetch(cfg!.status, { cache: "no-store" });
    const s = (await res.json())?.icestats?.source;
    const raw: unknown = Array.isArray(s) ? s[0]?.title : s?.title;
    title = typeof raw === "string" ? raw.replace(/&#(\d+);/g, (_, n) => String.fromCodePoint(Number(n))) : "";
  } catch {
    title = "";
  }
  renderBgm();
}

function renderBgm(): void {
  const btn = $("#bgm");
  btn.hidden = cfg == null;
  const text = !app.settings.bgm ? "BGM OFF" : blocked ? `BGM ON (${tapWord}で開始)` : "BGM ON";
  if (btn.textContent !== text) btn.textContent = text;
  btn.classList.toggle("active", app.settings.bgm && !blocked);
  btn.classList.toggle("waiting", app.settings.bgm && blocked);
  const on = !audio.paused;
  const now = on ? `BGM: ${title || "再生中"}` : "";
  const label = $("#bgm-now");
  label.hidden = !on;
  const inner = label.querySelector<HTMLElement>(".bgm-text")!;
  // 曲名や幅が変わったときだけ、収まらなければ流すかを判定し直す
  const sig = `${now}|${label.clientWidth}`;
  if (label.dataset.sig !== sig) {
    label.dataset.sig = sig;
    inner.textContent = now;
    const over = inner.scrollWidth - label.clientWidth;
    label.classList.toggle("scroll", on && over > 0);
    label.style.setProperty("--bgm-shift", `${-over}px`);
    label.style.setProperty("--bgm-sec", `${8 + Math.round(over / 15)}s`);
  }
  btn.title = on && title ? `再生中: ${title}` : "平時に BGM を流します (地震の表示の間は止まります)";
}

// 配信が途切れたら (送り出しの再起動など) 少し待ってつなぎ直す
for (const ev of ["error", "ended"]) {
  audio.addEventListener(ev, () => {
    // src が無いのは止めたとき (受信をやめた後始末) なので、つなぎ直さない
    if (!wanted() || !audio.getAttribute("src")) return;
    retryAt = Date.now() + RETRY_MS;
    stop();
  });
}

// ボタンは利用者の操作なので、その場で再生を始める (自動再生の制限を解く)
$("#bgm").addEventListener("click", () => {
  const on = !app.settings.bgm || blocked;
  app.settings = { ...app.settings, bgm: on };
  saveSettings(app.settings);
  blocked = false;
  retryAt = 0;
  if (!on) stop();
  else void start();
  renderBgm();
});

const volume = $<HTMLInputElement>("#bgm-volume");
volume.value = String(app.settings.bgmVolume);
volume.addEventListener("input", () => {
  app.settings = { ...app.settings, bgmVolume: Number(volume.value) };
  saveSettings(app.settings);
  clearInterval(fadeTimer);
  audio.volume = level();
});

// 前回 BGM を ON にしていて自動再生を止められたときは、画面のどこかを操作したら流す
document.addEventListener("pointerdown", (e) => {
  // BGM ボタン自体の操作はボタンの処理に任せる
  if ((e.target as Element).closest("#bgm")) return;
  if (blocked && app.settings.bgm) {
    blocked = false;
    void start();
  }
});
