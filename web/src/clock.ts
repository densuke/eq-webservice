// 日時表示用に、時刻を日本時間の年・月・日・曜日・時分・秒に分ける。

const JST_MS = 9 * 3600_000;
const WEEKDAYS = "日月火水木金土";

export interface ClockParts {
  year: string;
  month: string;
  day: string;
  weekday: string;
  hm: string;
  sec: string;
}

export function clockParts(ms: number): ClockParts {
  // UTC の関数で日本時間を読む (ブラウザのタイムゾーン設定に左右されない)
  const d = new Date(ms + JST_MS);
  const p2 = (n: number) => String(n).padStart(2, "0");
  return {
    year: String(d.getUTCFullYear()),
    month: p2(d.getUTCMonth() + 1),
    day: p2(d.getUTCDate()),
    weekday: WEEKDAYS[d.getUTCDay()],
    hm: `${p2(d.getUTCHours())}:${p2(d.getUTCMinutes())}`,
    sec: p2(d.getUTCSeconds()),
  };
}
