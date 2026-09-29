// 平時の地図に重ねる天気: アメダスの雨の地点 (強さで色分けした点) と、主要都市の天気の絵文字と気温。

import { map } from "./dom.ts";
import { el, project } from "./map.ts";
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
  map.rainLayer.replaceChildren(...(w ? rainPaths(w.rain) : []));
  map.cityLayer.replaceChildren(...(w ? w.cities.map((c) => cityMarker(c, night)) : []));
  map.refreshCities();
}

/** 雨の点は色ごとに 1 本の path にまとめる (長さ 0 の線を丸い線端で描くと、ズームしても同じ大きさの点になる) */
function rainPaths(rain: CityWeather["rain"]): SVGPathElement[] {
  const byColor = new Map<string, string>();
  for (const [lat, lon, mm] of [...rain].sort((a, b) => a[2] - b[2])) {
    const [x, y] = project(lon, lat);
    const c = rainColor(mm);
    byColor.set(c, (byColor.get(c) ?? "") + `M${x.toFixed(1)} ${y.toFixed(1)}h0`);
  }
  return [...byColor].map(([color, d]) => {
    const p = el("path", { d, class: "rain-dot" });
    p.style.stroke = color;
    return p;
  });
}

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
  // 点の上に「絵文字 気温」の札を出す
  const w = 22 + temp.length * 8;
  const box = el("rect", { x: -w / 2, y: -30, width: w, height: 22, rx: 11, class: "city-box" });
  const text = el("text", { x: 0, y: -19, "text-anchor": "middle", "dominant-baseline": "central", class: "city-text" });
  text.textContent = `${icon}${temp}`;
  g.append(title, el("circle", { r: 3, class: "city-dot" }), box, text);
  return g;
}
