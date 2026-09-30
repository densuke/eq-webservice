// 平時の地図に重ねる天気: アメダスの雨の地点 (強さで色分けした点) と、主要都市の天気の絵文字と気温。

import { map } from "./dom.ts";
import { dotPaths, el, project } from "./map.ts";
import { app } from "./state.ts";
import { type CityWeather, rainColor, rangeLabel, showTomorrow, tempLabel, weatherIcon } from "./weather.ts";

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
  const hour = new Date(now + 9 * 3600_000).getUTCHours();
  const night = hour < 6 || hour >= 18;
  // 札を「今」と「明日」で交互に出す (明日の予報がある都市だけ。切り替わった秒に描き直す)
  const tomorrow = w != null && showTomorrow(now, app.settings.weatherFlipSec);
  const next = w ? `${w.observed_at}|${night}|${tomorrow}` : "";
  if (next === sig) return;
  sig = next;
  // 雨の強い地点ほど上に
  const rain = w ? [...w.rain].sort((a, b) => a[2] - b[2]).map(([lat, lon, mm]) => ({ lat, lon, color: rainColor(mm) })) : [];
  map.rainLayer.replaceChildren(...dotPaths(rain, "rain-dot"));
  map.cityLayer.replaceChildren(...(w ? w.cities.map((c) => cityMarker(c, night, tomorrow)) : []));
  map.refreshCities();
}

/** 札の向き (無ければ上)。大阪と神戸、東京と千葉は近いので左右に分ける */
const SIDE: Record<string, "up" | "down" | "left" | "right"> = { 神戸: "left", 大阪: "right", 東京: "left", 千葉: "right", 高知: "down" };

/** 明日の札の向き。今より幅も高さもあるので、近い都市どうしが重ならないよう一部を変える (native/data.rs の city_side_tomorrow と同じ) */
const SIDE_TOMORROW: typeof SIDE = { ...SIDE, 新潟: "left", 福岡: "left", 東京: "up", 千葉: "down", 広島: "down" };

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
  // 明日の札は 2 段にして幅を抑える: 上に「絵文字 最高/最低」、下に小さい「明日」と降水確率
  const range = t ? rangeLabel(t.temp_max, t.temp_min) : "";
  const w = t ? 28 + range.length * 6.6 : 22 + temp.length * 8;
  const h = t ? 32 : 22;
  const [bx, by] = { up: [-w / 2, -8 - h], down: [-w / 2, 8], left: [-w - 7, -h / 2], right: [7, -h / 2] }[(t ? SIDE_TOMORROW : SIDE)[c.name] ?? "up"];
  const box = el("rect", { x: bx, y: by, width: w, height: h, rx: t ? 10 : 11, class: "city-box" });
  const text = el("text", { x: bx + w / 2, y: by + 11, "text-anchor": "middle", "dominant-baseline": "central", class: "city-text" });
  g.append(title, el("circle", { r: 3, class: "city-dot" }), box, text);
  if (!t) {
    text.textContent = `${icon}${temp}`;
    return g;
  }
  text.textContent = `${icon}${range}`;
  const sub = (x: number, anchor: string, cls: string, s: string) => {
    const e = el("text", { x, y: by + 24, "text-anchor": anchor, "dominant-baseline": "central", class: cls });
    e.textContent = s;
    return e;
  };
  g.append(sub(bx + 8, "start", "city-sub", "明日"), ...(t.pop != null ? [sub(bx + w - 8, "end", "city-pop", `${t.pop}%`)] : []));
  return g;
}
