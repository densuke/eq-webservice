// 平時の地図に重ねる天気: アメダスの雨の地点 (強さで色分けした点) と、主要都市の天気の絵文字と気温。

import { map } from "./dom.ts";
import { dotPaths, el, project } from "./map.ts";
import { type CityWeather, rainColor, tempLabel, weatherIcon } from "./weather.ts";

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
  const next = w ? `${w.observed_at}|${night}` : "";
  if (next === sig) return;
  sig = next;
  // 雨の強い地点ほど上に
  const rain = w ? [...w.rain].sort((a, b) => a[2] - b[2]).map(([lat, lon, mm]) => ({ lat, lon, color: rainColor(mm) })) : [];
  map.rainLayer.replaceChildren(...dotPaths(rain, "rain-dot"));
  map.cityLayer.replaceChildren(...(w ? w.cities.map((c) => cityMarker(c, night)) : []));
  map.refreshCities();
}

/** 札の向き (無ければ上)。大阪と神戸、東京と千葉は近いので左右に分ける */
const SIDE: Record<string, "up" | "down" | "left" | "right"> = { 神戸: "left", 大阪: "right", 東京: "left", 千葉: "right", 高知: "down" };

function cityMarker(c: CityWeather["cities"][number], night: boolean): SVGGElement {
  const [x, y] = project(c.lon, c.lat);
  const g = el("g", { class: "city" });
  g.dataset.x = String(x);
  g.dataset.y = String(y);
  g.dataset.k = "1";
  const icon = weatherIcon(c.code, c.text, night);
  const temp = tempLabel(c.temp);
  const title = el("title");
  const rain = c.precip1h ? ` / 1時間降水量 ${c.precip1h}mm` : "";
  title.textContent = `${c.name}: ${c.text || "天気不明"}${c.temp != null ? ` / ${c.temp}℃` : ""}${rain}`;
  // 点のそばに「絵文字 気温」の札を出す (隣り合う都市とは向きを変えて重ならないように)
  const w = 22 + temp.length * 8;
  const [bx, by] = { up: [-w / 2, -30], down: [-w / 2, 8], left: [-w - 7, -11], right: [7, -11] }[SIDE[c.name] ?? "up"];
  const box = el("rect", { x: bx, y: by, width: w, height: 22, rx: 11, class: "city-box" });
  const text = el("text", { x: bx + w / 2, y: by + 11, "text-anchor": "middle", "dominant-baseline": "central", class: "city-text" });
  text.textContent = `${icon}${temp}`;
  g.append(title, el("circle", { r: 3, class: "city-dot" }), box, text);
  return g;
}
