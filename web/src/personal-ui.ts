// 自分の地点・通知の設定画面、ブラウザ通知、主要動の到達カウントダウン。

import { eewAreaScales, quakeDetail } from "./detail.ts";
import { esc } from "./html.ts";
import { type NotifyLevel, countdown, countdownWorthShowing, estimateIntensity, intensityToScale, loadSettings, nearestArea, notifyScale, saveSettings, shouldNotify } from "./personal.ts";
import { activeEews } from "./quakes.ts";
import { scaleLabel } from "./scale.ts";
import { $, app, map } from "./state.ts";
import type { EqEvent } from "./types.ts";
import { GRADE_LABEL } from "./view.ts";

export let settings = loadSettings();

/** 自分の地点が属する細分区域 (地点に最も近い震度観測点の区域) */
export let homeArea: string | null = null;

/** 通知済みの地震 (グループのキー → 通知したときの深刻さ)。深刻さが上がったときだけまた通知する */
export const notified = new Map<string, number>();

export function updateHome(): void {
  homeArea = settings.home ? nearestArea(settings.home, app.stations) : null;
  map.setHome(settings.home);
  renderSettings();
}

export function renderSettings(): void {
  const h = settings.home;
  $("#home-label").textContent = h ? `${homeArea ?? "地点"} (北緯${h.lat.toFixed(2)} 東経${h.lon.toFixed(2)})` : "未設定";
  $<HTMLSelectElement>("#notify-level").value = settings.notify;
}

export function setSettings(next: typeof settings): void {
  settings = next;
  saveSettings(settings);
  updateHome();
}

/** 自分の地点の震度 (緊急地震速報は予測、地震情報は観測) */
export function homeScaleOf(e: EqEvent): number | null {
  if (!homeArea) return null;
  if (e.kind === "eew") return eewAreaScales(e.areas).find((a) => a.name === homeArea)?.scale ?? null;
  if (e.kind === "quake") return quakeDetail(e.points, app.stations).areas.find((a) => a.name === homeArea)?.scale ?? null;
  return null;
}

export function notify(e: EqEvent): void {
  if (!("Notification" in window) || Notification.permission !== "granted" || document.visibilityState === "visible") return;
  if ((e.kind === "eew" && (e.test || e.cancelled)) || (e.kind === "tsunami" && e.cancelled)) return;
  let warning = false;
  let maxScale = 0;
  let title = "";
  let body = "";
  if (e.kind === "eew") {
    [warning, maxScale] = [e.warning, e.max_scale];
    title = `緊急地震速報 (${e.warning ? "警報" : "予報"})`;
    body = `${e.hypocenter?.name ?? "震源不明"} で地震 ・ 予測最大震度${scaleLabel(e.max_scale)}`;
  } else if (e.kind === "quake") {
    maxScale = e.max_scale;
    title = "地震情報";
    body = `${e.hypocenter?.name || "震源調査中"} ・ 最大震度${scaleLabel(e.max_scale)}`;
  } else if (e.kind === "tsunami") {
    maxScale = Math.max(0, ...e.areas.map((a) => notifyScale.tsunami(a.grade)));
    warning = maxScale >= notifyScale.tsunami("warning");
    title = GRADE_LABEL[e.areas.find((a) => notifyScale.tsunami(a.grade) === maxScale)?.grade ?? "unknown"];
    body = e.areas.map((a) => a.name).join("・");
  } else return;
  const homeScale = homeScaleOf(e);
  if (!shouldNotify(settings.notify, { warning, maxScale, homeScale })) return;
  const key = app.world.store.list().find((g) => g.events.some((x) => x.id === e.id))?.key ?? e.id;
  const severity = Math.max(maxScale, homeScale ?? 0) + (warning ? 100 : 0);
  if ((notified.get(key) ?? -1) >= severity) return;
  notified.set(key, severity);
  if (homeScale != null) body += ` ・ ${homeArea}: 震度${scaleLabel(homeScale)}`;
  const n = new Notification(title, { body, tag: key });
  n.onclick = () => window.focus();
}

/** 自分の地点に主要動が届くまで。緊急地震速報を受けている間だけ出す */
export function renderCountdown(now: number): void {
  const el = $("#countdown");
  const h = settings.home;
  const cands = h
    ? activeEews(now).filter((e) => e.hypocenter?.latitude != null && e.hypocenter.longitude != null && e.origin_time_ms != null)
    : [];
  // 遠くて揺れそうにない地震は出さない: 自分の地点の区域が緊急地震速報に含まれているか、
  // 距離減衰式で推定した震度が 3 以上のときだけ
  const best = cands
    .map((x) => {
      const hy = x.hypocenter!;
      const c = countdown(h!, { lat: hy.latitude!, lon: hy.longitude!, depth: hy.depth_km ?? 10 }, x.origin_time_ms!, now);
      const est = hy.magnitude != null ? estimateIntensity(hy.magnitude, hy.depth_km ?? 10, c.distKm) : null;
      return { s: homeScaleOf(x), est, c };
    })
    .filter((x) => countdownWorthShowing(x.s, x.est))
    // 自分の地点の予測震度 (無ければ推定) が大きいもの、同じなら先に揺れが届くもの
    .sort((a, b) => (b.s ?? intensityToScale(b.est!)) - (a.s ?? intensityToScale(a.est!)) || a.c.remainingSec - b.c.remainingSec)[0];
  el.hidden = !best;
  if (!best) return;
  const { c, s, est } = best;
  const scaleText = s != null ? `予測震度${scaleLabel(s)}` : `推定震度${scaleLabel(intensityToScale(est!))} (概算)`;
  el.classList.toggle("arrived", c.arrived);
  const html = `${esc(homeArea ?? "自分の地点")} ・ ${scaleText}<div class="cd-sec">${
    c.arrived ? "揺れが到達したと推定" : `あと ${Math.ceil(c.remainingSec)} 秒`
  }</div><div class="cd-sub">主要動 (S波) の到達までの概算 ・ 震央から ${c.distKm.toFixed(0)} km</div>`;
  if (el.innerHTML !== html) el.innerHTML = html;
}

$("#settings-open").addEventListener("click", () => {
  const p = $("#settings-panel");
  p.hidden = !p.hidden;
  renderSettings();
});

$("#home-pick").addEventListener("click", () => {
  $("#settings-note").textContent = "地図をタップ (クリック) して、自分の地点を選んでください。";
  map.pickPoint((p) => {
    $("#settings-note").textContent = "地点と設定はこの端末の中だけに保存されます。通知はこの画面が裏にあるときに出ます。";
    setSettings({ ...settings, home: p });
  });
});

$("#home-geo").addEventListener("click", () => {
  if (!navigator.geolocation) return;
  navigator.geolocation.getCurrentPosition(
    (pos) => setSettings({ ...settings, home: { lat: pos.coords.latitude, lon: pos.coords.longitude } }),
    () => ($("#settings-note").textContent = "位置情報を取得できませんでした。地図で選んでください。"),
    { timeout: 10_000 },
  );
});

$("#home-clear").addEventListener("click", () => setSettings({ ...settings, home: null }));

$("#notify-level").addEventListener("change", (e) => {
  const level = (e.target as HTMLSelectElement).value as NotifyLevel;
  setSettings({ ...settings, notify: level });
  // 通知を使うなら、この操作の中で許可を求める
  if (level !== "off" && "Notification" in window && Notification.permission === "default") void Notification.requestPermission();
});
