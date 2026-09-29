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
