// 平時の地図に重ねる天気: アメダスの雨の地点 (強さで色分けした点) と、主要都市の天気の絵文字と気温。

import { $, map } from "./dom.ts";
import { dotPaths, el, project } from "./map.ts";
import { app } from "./state.ts";
import { WEATHER_OFF } from "./personal.ts";
import { type CityWeather, rainColor, rangeLabel, tempLabel, weatherCaption, weatherIcon, weatherView } from "./weather.ts";

let data: CityWeather | null = null;
let sig = "";

/** サーバは 10 分ごとにアメダスを取得している */
export async function loadCityWeather(): Promise<void> {
  const res = await fetch("api/weather");
  if (res.ok) data = await res.json();
}

/** 平時 (show) だけ出し、地震の表示の間は消す */
export function renderCityWeather(show: boolean, now: number): void {
  const w = show ? data : null;
  // 札だけ消す設定 (雨の点は残す)。案内も札と一緒に消す
  const badges = w && app.settings.weatherFlipSec !== WEATHER_OFF ? w : null;
  const hour = new Date(now + 9 * 3600_000).getUTCHours();
  const night = hour < 6 || hour >= 18;
  // 札を「今」と「明日」で交互に出す (明日の予報がある都市だけ。明日の予報がまだ無いうちは今のまま)。
  // 案内も同じ種類から決めて、札と同じ回に描く (切り替わった秒に描き直す)
  const view = badges?.cities.some((c) => c.tomorrow) ? weatherView(now, app.settings.weatherFlipSec) : "now";
  const caption = badges ? weatherCaption(view, now) : "";
  const tomorrow = view === "tomorrow";
  const next = w ? `${w.observed_at}|${night}|${caption}|${badges != null}` : "";
  // 凡例の行や時計の大きさは地図の外で変わるので、描き直さない回も間引きだけはやり直す
  if (next === sig) return map.thinCities();
  sig = next;
  const box = $("#weather-caption");
  box.hidden = !badges;
  box.textContent = caption;
  // 雨の強い地点ほど上に
  const rain = w ? [...w.rain].sort((a, b) => a[2] - b[2]).map(([lat, lon, mm]) => ({ lat, lon, color: rainColor(mm) })) : [];
  map.rainLayer.replaceChildren(...dotPaths(rain, "rain-dot"));
  map.cityLayer.replaceChildren(...(badges ? [...badges.cities].sort((a, b) => rank(a.name) - rank(b.name)).map((c) => cityMarker(c, night, tomorrow)) : []));
  map.refreshCities();
}

/**
 * 札を置く優先の順。狭い画面で重なるときは、後ろの都市の札から出さない。
 * 人口と、天気を見たい人の多さで大都市を先に (東京・大阪・名古屋)、次に地方の中心 (札幌・福岡・那覇・仙台・広島・新潟)。
 * 東京・大阪の近所の千葉・神戸と、福岡の近所の鹿児島・高知は、近くの大都市と同じ天気を読めるので最後
 */
const PRIORITY = ["東京", "大阪", "札幌", "福岡", "那覇", "名古屋", "仙台", "広島", "新潟", "鹿児島", "高知", "千葉", "神戸"];
const rank = (name: string) => PRIORITY.indexOf(name) + 1 || PRIORITY.length + 1;

/** 札の向き (無ければ上)。大阪と神戸、東京と千葉は近いので左右に分ける */
const SIDE: Record<string, "up" | "down" | "left" | "right"> = { 神戸: "left", 大阪: "right", 東京: "left", 千葉: "right", 高知: "down" };

/** 明日の札の向き。今より幅も高さもあるので、近い都市どうしが重ならないよう一部を変える (地図の大きさが違うので、native の city_side_tomorrow とは別に決めている) */
const SIDE_TOMORROW: typeof SIDE = { ...SIDE, 仙台: "down", 福岡: "down", 鹿児島: "down" };

function cityMarker(c: CityWeather["cities"][number], night: boolean, tomorrow: boolean): SVGGElement {
  const [x, y] = project(c.lon, c.lat);
  const g = el("g", { class: "city" });
  g.dataset.x = String(x);
  g.dataset.y = String(y);
  g.dataset.k = "1";
  const t = tomorrow ? c.tomorrow : null;
  const icon = t ? weatherIcon(t.code, t.text, false) : weatherIcon(c.code, c.text, night);
  const temp = tempLabel(c.temp);
  const title = el("title");
  const rain = c.precip1h ? ` / 1時間降水量 ${c.precip1h}mm` : "";
  title.textContent = t
    ? `${c.name}の明日: ${t.text}${t.temp_max != null ? ` / 最高 ${t.temp_max}℃` : ""}${t.temp_min != null ? ` / 最低 ${t.temp_min}℃` : ""}${t.pop != null ? ` / 降水確率 ${t.pop}%` : ""}`
    : `${c.name}: ${c.text || "天気不明"}${c.temp != null ? ` / ${c.temp}℃` : ""}${rain}`;
  // 点のそばに「絵文字 気温」の札を出す (隣り合う都市とは向きを変えて重ならないように)。
  // 明日の札は幅を抑えるため、「絵文字 最高/最低」の下に小さく降水確率を置く 2 段にする
  const range = t ? rangeLabel(t.temp_max, t.temp_min) : "";
  const pop = t?.pop != null ? `${t.pop}%` : "";
  const w = t ? 28 + range.length * 6.6 : 22 + temp.length * 8;
  const h = pop ? 32 : 22;
  const [bx, by] = { up: [-w / 2, -8 - h], down: [-w / 2, 8], left: [-w - 7, -h / 2], right: [7, -h / 2] }[(t ? SIDE_TOMORROW : SIDE)[c.name] ?? "up"];
  const line = (y: number, cls: string, s: string) => {
    const e = el("text", { x: bx + w / 2, y: by + y, "text-anchor": "middle", "dominant-baseline": "central", class: cls });
    e.textContent = s;
    return e;
  };
  // 札 (箱と文字) は badge にまとめる。重なって動かすときは badge ごと動かし、点から引き出し線 (leader) でつなぐ
  const badge = el("g", { class: "city-badge" });
  badge.append(el("rect", { x: bx, y: by, width: w, height: h, rx: pop ? 10 : 11, class: "city-box" }), line(11, "city-text", t ? `${icon}${range}` : `${icon}${temp}`));
  if (pop) badge.append(line(24, "city-pop", pop));
  g.append(title, el("line", { class: "city-leader", visibility: "hidden" }), el("circle", { r: 3, class: "city-dot" }), badge);
  return g;
}
