// 平時に地図へ重ねる主要都市の天気と、アメダスの雨の地点 (GET /api/weather)。出典: 気象庁

interface City {
  name: string;
  lat: number;
  lon: number;
  /** 天気予報の天気コード (100 = 晴れ など)。取れていなければ空 */
  code: string;
  /** "くもり時々雨" など */
  text: string;
  temp: number | null;
  precip1h: number | null;
  /** 明日の予報 (古いサーバ・取れていなければ無い) */
  tomorrow?: Tomorrow | null;
}

export interface Tomorrow {
  code: string;
  text: string;
  temp_min: number | null;
  temp_max: number | null;
  /** 降水確率 (%) */
  pop: number | null;
}

export interface CityWeather {
  observed_at: string;
  cities: City[];
  /** 1 時間降水量が 1mm 以上の地点 [緯度, 経度, mm] */
  rain: [number, number, number][];
}

/**
 * 天気の絵文字。天気コードの百の位が主な天気 (1 晴れ / 2 くもり / 3 雨 / 4 雪)、文から時々・一時の天気を読む。
 * 夜 (night) の晴れは月にする
 */
export function weatherIcon(code: string, text: string, night: boolean): string {
  const main = code[0];
  if (text.includes("雷")) return "⛈️";
  if (main === "4") return text.includes("雨") ? "🌨️" : "❄️";
  if (main === "3") return text.includes("雪") ? "🌨️" : "☔";
  if (main === "2") return text.includes("雪") ? "🌨️" : text.includes("雨") ? "🌧️" : text.includes("晴") ? "⛅" : "☁️";
  if (main === "1") {
    if (text.includes("雨")) return "🌦️";
    if (text.includes("雪")) return "🌨️";
    if (text.includes("くもり")) return night ? "☁️" : "🌤️";
    return night ? "🌙" : "☀️";
  }
  return "";
}

/** 1 時間降水量の色 (気象庁の降水の配色に合わせる) */
export function rainColor(mm: number): string {
  if (mm >= 80) return "#b40068";
  if (mm >= 50) return "#ff2800";
  if (mm >= 30) return "#ff9900";
  if (mm >= 20) return "#faf500";
  if (mm >= 10) return "#0041ff";
  if (mm >= 5) return "#218cff";
  if (mm >= 1) return "#a0d2ff";
  return "#f2f2ff";
}

/** 気温の表示 ("22°")。無ければ空 */
export function tempLabel(t: number | null): string {
  return t == null ? "" : `${Math.round(t)}°`;
}

/** 最高/最低気温の表示 ("24°/17°")。片方しか無ければ "24°/-"、両方無ければ空 */
export function rangeLabel(max: number | null, min: number | null): string {
  if (max == null && min == null) return "";
  const one = (t: number | null) => (t == null ? "-" : tempLabel(t));
  return `${one(max)}/${one(min)}`;
}

/** 天気の札が出す内容の種類。切り替えの並びは WEATHER_VIEWS (crates/eq-server/src/broadcast/native/data.rs の WEATHER_VIEWS と同じ) */
export type WeatherView = "now" | "tomorrow";
const WEATHER_VIEWS: WeatherView[] = ["now", "tomorrow"];

/** 今出す種類 (flipSec 秒ごとに並びを順に回す。0 なら先頭のまま)。native/data.rs の weather_view と同じ */
export function weatherView(now: number, flipSec: number): WeatherView {
  return flipSec > 0 ? WEATHER_VIEWS[Math.floor(now / 1000 / flipSec) % WEATHER_VIEWS.length] : WEATHER_VIEWS[0];
}

/** 何を出しているかの案内。明日は日付を添える ("明日 10/2 (金) の天気")。native/data.rs の weather_caption と同じ */
export function weatherCaption(view: WeatherView, now: number): string {
  if (view === "now") return "現在の天気";
  const d = new Date(now + 33 * 3600_000); // 日本時間の明日 (UTC+9 に 1 日)
  return `明日 ${d.getUTCMonth() + 1}/${d.getUTCDate()} (${"日月火水木金土"[d.getUTCDay()]}) の天気`;
}
