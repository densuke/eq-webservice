// 画面の文字情報: 一覧・詳細・緊急地震速報と津波予報のバナー・表示モード、地図の塗り分け。

import { type AreaScale, eewAreaScales, keepForecast, overlayForecast, quakeDetail } from "./detail.ts";
import { type EewGroup, type Group, heldEew, latestEew, summarizeQuake } from "./groups.ts";
import { esc } from "./html.ts";
import { byPriority, sameQuake } from "./priority.ts";
import { activeEews, currentGroup, groupPlace, groupScale, relatedQuake } from "./quakes.ts";
import { scaleColor, scaleLabel, scaleTextColor } from "./scale.ts";
import { $, map } from "./dom.ts";
import { listOpen, shownInList } from "./personal.ts";
import { app, now } from "./state.ts";
import { activeAreas } from "./tsunami.ts";
import type { Hypocenter, PrefScale, Scale, TsunamiEvent } from "./types.ts";

/** EEW バナーに並べる件数 (残りは「ほか N 件」) */
const EEW_BANNER_MAX = 3;
/** 帯を 1 件ずつ見せる段階 (data-variant="compact"、横向きのスマホ) で次の EEW へ切り替える間隔 */
const EEW_ROTATE_MS = 4000;

export function numTag(key: string): string {
  const n = app.numbers.get(key);
  return n == null ? "" : `<span class="num">${n}</span>`;
}

function badge(s: Scale, big = false): string {
  return `<span class="badge${big ? " big" : ""}" style="background:${scaleColor(s)};color:${scaleTextColor(s)}">${
    s > 0 ? scaleLabel(s) : "-"
  }</span>`;
}

function hypoText(h: Hypocenter | null): string {
  if (!h) return "震源調査中";
  const parts = [esc(h.name || "震源不明")];
  if (h.magnitude != null) parts.push(`M${h.magnitude.toFixed(1)}`);
  if (h.depth_km === 0) parts.push("ごく浅い");
  else if (h.depth_km != null) parts.push(`深さ${h.depth_km}km`);
  return parts.join(" / ");
}

const TSUNAMI_TEXT: Record<string, string> = {
  None: "この地震による津波の心配はありません",
  NonEffective: "若干の海面変動 (被害の心配なし)",
  Checking: "津波の有無を調査中",
  Watch: "津波注意報 発表中",
  Warning: "津波警報等 発表中",
};

export const GRADE_LABEL: Record<string, string> = {
  major_warning: "大津波警報",
  warning: "津波警報",
  watch: "津波注意報",
  unknown: "不明",
};

function groupRow(g: Group): string {
  switch (g.kind) {
    case "quake": {
      const q = summarizeQuake(g);
      return `${badge(q.maxScale)}<div class="row-main"><div class="row-title">${numTag(g.key)}${esc(
        q.hypocenter?.name || "震源調査中",
      )}</div><div class="row-sub">${esc(q.originTime.slice(5, 16))} ${q.hypocenter?.magnitude != null ? "M" + q.hypocenter.magnitude.toFixed(1) : ""} ・${q.infoLabel}</div></div>`;
    }
    case "eew": {
      const e = heldEew(g);
      return `${badge(e.max_scale)}<div class="row-main"><div class="row-title eew-title">${numTag(g.key)}${e.test ? "[テスト] " : ""}緊急地震速報${e.warning ? "" : " (予報)"} ${
        e.cancelled ? "(取消)" : esc(e.hypocenter?.name ?? "")
      }</div><div class="row-sub">${esc((e.origin_time ?? e.issued_at).slice(5, 16))} ・第${esc(e.serial)}報</div></div>`;
    }
    case "tsunami": {
      const t = g.events[0] as TsunamiEvent;
      const top = t.areas[0]?.grade ?? "unknown";
      return `<span class="badge tsunami ${top}">津</span><div class="row-main"><div class="row-title">${
        t.cancelled ? "津波予報 解除" : GRADE_LABEL[top]
      }</div><div class="row-sub">${esc(t.issued_at.slice(5, 16))} ・${t.areas.length}地域</div></div>`;
    }
    case "eew_detection":
      return `<span class="badge">!</span><div class="row-main"><div class="row-title">緊急地震速報 発表検出</div><div class="row-sub">${new Date(
        g.updatedAt,
      ).toLocaleTimeString("ja-JP", { timeZone: "Asia/Tokyo" })}</div></div>`;
  }
}

export function renderList(): void {
  const list = $("#list");
  // 設定の「履歴に出す地震」でしぼり込む (緊急地震速報・津波予報は常に出す)
  const groups = app.world.store
    .list()
    .filter((g) => g.kind !== "eew_detection" && shownInList(g.kind, groupScale(g), app.settings.listMin));
  const current = currentGroup();
  list.innerHTML = groups
    .slice(0, 100)
    .map((g) => `<li data-key="${esc(g.key)}" class="${g.key === current?.key ? "selected" : ""}">${groupRow(g)}</li>`)
    .join("");
  renderMode();
}

/** 今の表示モード (リアルタイム / リプレイ中 / デモモード中) と「リアルタイムに戻る」ボタン */
export function renderMode(): void {
  const mode = app.demo ? (app.demo.history ? "replay" : "demo") : app.selectedKey ? "replay" : app.tourKey ? "tour" : "live";
  const el = $("#mode");
  if (el.dataset.mode !== mode) {
    el.dataset.mode = mode;
    el.textContent = { live: "リアルタイム", replay: "リプレイ中", demo: "デモモード中", tour: "巡回中" }[mode];
  }
  const back = $("#back-live");
  back.hidden = (mode === "live" || mode === "tour") && !map.userMoved;
  // デモ中に地震を選んだり地図を動かしたりしたときは、デモを終えずに自動表示 (巡回) へ戻す
  const label = app.demo && (app.selectedKey || map.userMoved) ? "自動表示に戻る" : "リアルタイムに戻る";
  if (back.textContent !== label) back.textContent = label;
  $("#demo-open").hidden = app.demo != null;
}

const byName = (xs: PrefScale[]): AreaScale[] => xs.map(({ pref, scale }) => ({ name: pref, scale }));

/** 緊急地震速報の予測 (同じ地震の報の最大。報ごとに地域が出たり消えたりしても点滅させない) */
function forecastLayers(g: EewGroup): { prefs: AreaScale[]; areas: AreaScale[] } {
  const e = heldEew(g);
  if (e.cancelled) return { prefs: [], areas: [] };
  return { prefs: byName(e.pref_max), areas: eewAreaScales(e.areas) };
}

/** 地図の塗り分けと震央 */
function paintMap(g: Group | undefined): void {
  if (g?.kind === "quake") {
    const q = summarizeQuake(g);
    const d = quakeDetail(q.points, app.stations);
    // 同じ地震の緊急地震速報の予測を重ねて残す (観測の無い地域は予測のまま)。観測の震度が届くまでは速報が終わっても残す
    const active = new Set(activeEews(now()).map((e) => e.event_id));
    const observed = q.points.length > 0;
    const eg = app.world.store
      .list()
      .find(
        (x): x is EewGroup =>
          x.kind === "eew" &&
          !latestEew(x).cancelled &&
          sameQuake(groupPlace(x), groupPlace(g)) &&
          keepForecast(active.has(latestEew(x).event_id), observed, now() - latestEew(x).received_at_ms),
      );
    const f = eg ? forecastLayers(eg) : { prefs: [], areas: [] };
    const prefs = overlayForecast(byName(q.prefMax), f.prefs);
    map.setPrefScales(prefs.map(({ name, ...rest }) => ({ pref: name, ...rest })));
    map.setDetail(overlayForecast(d.areas, f.areas), false, d.dots);
  } else if (g?.kind === "eew") {
    const f = forecastLayers(g);
    map.setPrefScales(f.prefs.map(({ name, ...rest }) => ({ pref: name, ...rest })), true);
    map.setDetail(f.areas, true, []);
  } else {
    map.setPrefScales([]);
    map.setDetail([], false, []);
  }
}

/** 観測点の一覧を開いておくか (最後の発表から設定の時間で畳む。利用者が開閉していればそれに従う) */
function pointsOpen(g: Group): boolean {
  return listOpen(app.settings.collapseMin, g.updatedAt, now(), app.listOpen.get(g.key));
}

/** 時間がたって一覧を畳む時刻になったら畳む (描き直さずに開閉だけ変える) */
export function updatePointsOpen(): void {
  const el = document.querySelector<HTMLDetailsElement>("#detail details.points-box");
  const g = el?.dataset.key ? app.world.store.get(el.dataset.key) : undefined;
  if (el && g && el.open !== pointsOpen(g)) el.open = pointsOpen(g);
}

export function renderDetail(): void {
  const g = currentGroup();
  const box = $("#detail");
  paintMap(g && relatedQuake(g));
  if (!g) {
    box.innerHTML = `<p class="muted">受信した情報はまだありません。</p>`;
    return;
  }
  if (g.kind === "quake") {
    const q = summarizeQuake(g);
    // 震度の大きい順に観測点をまとめる
    const byScale = new Map<Scale, Map<string, string[]>>();
    for (const p of q.points) {
      const prefs = byScale.get(p.scale) ?? new Map<string, string[]>();
      byScale.set(p.scale, prefs);
      prefs.set(p.pref, [...(prefs.get(p.pref) ?? []), p.addr]);
    }
    const scales = [...byScale.keys()].sort((a, b) => b - a);
    box.innerHTML = `
      <div class="detail-head">${badge(q.maxScale, true)}
        <div><div class="detail-kind">${q.infoLabel}</div>
        <div class="detail-title">${numTag(g.key)}${esc(q.hypocenter?.name || "震源調査中")}</div>
        <div class="detail-sub">${esc(q.originTime)} 発生</div></div></div>
      <dl class="facts">
        <dt>震源</dt><dd>${hypoText(q.hypocenter)}</dd>
        <dt>津波</dt><dd>${esc(TSUNAMI_TEXT[q.domesticTsunami] ?? "—")}</dd>
      </dl>
      ${q.comment ? `<p class="comment">${esc(q.comment)}</p>` : ""}
      ${
        scales.length
          ? `<details class="points-box" data-key="${esc(g.key)}"${pointsOpen(g) ? " open" : ""}><summary>${scales
              .map((s) => {
                const n = [...byScale.get(s)!.values()].reduce((a, v) => a + v.length, 0);
                return `<span class="sum-item">${badge(s)}${n}${q.points.some((p) => p.is_area) ? "地域" : "地点"}</span>`;
              })
              .join("")}<span class="sum-more">一覧</span></summary>`
          : ""
      }
      <div class="points">${scales
        .map(
          (s) =>
            `<div class="point-row">${badge(s)}<div>${[...byScale.get(s)!.entries()]
              .map(([pref, addrs]) => `<b>${esc(pref)}</b> ${esc(addrs.slice(0, 30).join("、"))}${addrs.length > 30 ? " ほか" : ""}`)
              .join("<br>")}</div></div>`,
        )
        .join("")}</div>${scales.length ? "</details>" : ""}`;
  } else if (g.kind === "eew") {
    const e = heldEew(g);
    box.innerHTML = `
      <div class="detail-head">${badge(e.max_scale, true)}
        <div><div class="detail-kind eew-title">緊急地震速報 (${e.warning ? "警報" : "予報"})${e.test ? " [テスト]" : ""} 第${esc(e.serial)}報</div>
        <div class="detail-title">${numTag(g.key)}${e.cancelled ? "取り消されました" : esc(e.hypocenter?.name ?? "震源不明")}</div>
        <div class="detail-sub">${esc(e.origin_time ?? e.issued_at)} 発生</div></div></div>
      <dl class="facts"><dt>震源</dt><dd>${hypoText(e.hypocenter)}</dd><dt>予測最大</dt><dd>震度${scaleLabel(e.max_scale)}</dd></dl>
      <div class="points">${e.areas
        .map(
          (a) =>
            `<div class="point-row">${badge(a.scale_from)}<div><b>${esc(a.name)}</b> 震度${scaleLabel(a.scale_from)}${
              a.scale_to == null ? "程度以上" : a.scale_to !== a.scale_from ? "〜" + scaleLabel(a.scale_to) : ""
            }${a.arrived ? ' <span class="arrived">到達と推測</span>' : ""}</div></div>`,
        )
        .join("")}</div>`;
  } else if (g.kind === "tsunami") {
    const t = g.events[0] as TsunamiEvent;
    box.innerHTML = `<div class="detail-head"><span class="badge big tsunami ${t.areas[0]?.grade ?? "unknown"}">津</span>
      <div><div class="detail-kind">津波予報</div><div class="detail-title">${t.cancelled ? "解除" : GRADE_LABEL[t.areas[0]?.grade ?? "unknown"]}</div>
      <div class="detail-sub">${esc(t.issued_at)} 発表</div></div></div>
      <div class="points">${t.areas
        .map(
          (a) =>
            `<div class="point-row"><span class="badge tsunami ${a.grade}">${GRADE_LABEL[a.grade].replace("津波", "")}</span><div><b>${esc(a.name)}</b>${
              a.immediate ? ' <span class="arrived">直ちに来襲</span>' : ""
            }${a.max_height ? ` 予想 ${esc(a.max_height)}` : ""}${a.first_height ? ` / ${esc(a.first_height)}` : ""}</div></div>`,
        )
        .join("")}</div>`;
  }
}

export function renderTsunamiBanner(): void {
  const areas = activeAreas(app.world.tsunami);
  const banner = $("#tsunami-banner");
  banner.hidden = areas.length === 0;
  if (areas.length === 0) return;
  const grades = (["major_warning", "warning", "watch"] as const).filter((g) => areas.some((a) => a.grade === g));
  banner.dataset.grade = grades[0] ?? "unknown";
  banner.innerHTML = grades
    .map((g) => `<b>${GRADE_LABEL[g]}</b> ${esc(areas.filter((a) => a.grade === g).map((a) => a.name).join("・"))}`)
    .join(" ／ ");
}

export function renderBanner(now: number): void {
  // 揺れの大きい順に数件だけ並べる
  const eews = activeEews(now)
    .map((e) => ({ e, scale: e.max_scale, at: e.received_at_ms }))
    .sort(byPriority)
    .map((c) => c.e);
  const banner = $("#eew-banner");
  banner.hidden = eews.length === 0;
  // 予報だけなら警報と色を分ける
  banner.classList.toggle("forecast", eews.length > 0 && eews.every((e) => !e.warning));
  const row = (e: (typeof eews)[number]) => {
    const prefs = e.pref_max.map((p) => p.pref).join("・");
    return `<div>${numTag(`e:${e.event_id}`)}<b>${e.test ? "【テスト】" : ""}緊急地震速報 (${e.warning ? "警報" : "予報"})</b> ${esc(e.hypocenter?.name ?? "")} で地震 ・ ${
      e.warning ? "強い揺れに警戒" : `予測最大震度${scaleLabel(e.max_scale)}`
    }: ${esc(prefs || "—")}</div>`;
  };
  // 1 件ずつ見せる段階: 揺れの大きい順に巡回し、何件目かを出す (色はいま見せている報で決める)
  if (banner.dataset.variant === "compact" && eews.length > 0) {
    const i = Math.floor(now / EEW_ROTATE_MS) % eews.length;
    banner.classList.toggle("forecast", !eews[i].warning);
    const html = (eews.length > 1 ? `<span class="eew-count">${i + 1}/${eews.length}</span>` : "") + row(eews[i]);
    if (banner.innerHTML !== html) banner.innerHTML = html;
    return;
  }
  const rest = eews.length - EEW_BANNER_MAX;
  banner.innerHTML = eews.slice(0, EEW_BANNER_MAX).map(row).join("") + (rest > 0 ? `<div>ほか ${rest} 件の緊急地震速報</div>` : "");
}
