import type { EqEvent } from "./types.ts";

export interface Item {
  id: string;
  group: string;
  /** 優先 (津波警報など)。待っている通常の読み上げより前に入る */
  priority?: boolean;
  /** POST /api/tts/announce に送る JSON。なければ id で GET */
  body?: string;
}

/** 大津波警報・津波警報を含む (解除でない) 津波か */
export function isPriorityTsunami(e: { kind: string; areas?: readonly object[]; cancelled?: boolean }): boolean {
  return e.kind === "tsunami" && !e.cancelled && !!e.areas?.some((a) => ["major_warning", "warning"].includes((a as { grade?: string }).grade ?? ""));
}

const MAX_PRIORS = 300;

/** POST /api/tts/announce の本文。priors は同じ地震の、この報より前の報 (最大 300) */
export function announceBody(e: EqEvent, groupEvents: readonly EqEvent[]): string {
  const i = groupEvents.findIndex((x) => x.id === e.id);
  const priors = (i >= 0 ? groupEvents.slice(0, i) : groupEvents).slice(-MAX_PRIORS);
  return JSON.stringify({ event: e, priors });
}

export function announceUrl(base: string): string {
  return new URL("api/tts/announce", base).href;
}

export const MAX_VOICES = 4;

/** 待ち行列に足した新しい配列を返す。同じ group が待っていれば (通常は) その位置で差し替え、溢れたら通常の古い方から捨てる */
export function enqueue(queue: readonly Item[], item: Item): Item[] {
  const i = queue.findIndex((q) => q.group === item.group);
  let next: Item[];
  if (i >= 0 && !item.priority) {
    next = queue.map((q, j) => (j === i ? item : q));
  } else {
    const rest = queue.filter((q) => q.group !== item.group);
    if (item.priority) {
      // 待っている通常の読み上げより前、優先の後ろに入る
      const at = rest.findIndex((q) => !q.priority);
      next = at >= 0 ? [...rest.slice(0, at), item, ...rest.slice(at)] : [...rest, item];
    } else next = [...rest, item];
  }
  while (next.length > MAX_VOICES) {
    const d = next.findIndex((q) => !q.priority);
    next = next.filter((_, j) => j !== (d >= 0 ? d : 0));
  }
  return next;
}

export function voiceUrl(id: string, base: string): string {
  return new URL("api/tts/event/" + encodeURIComponent(id), base).href;
}

let queue: Item[] = [];
let playing = false;

function next(): void {
  const [head, ...rest] = queue;
  queue = rest;
  if (!head) {
    playing = false;
    return;
  }
  playing = true;
  // 404 では error と play() の失敗が両方起きるので、次へ進むのは 1 回だけにする
  let done = false;
  const advance = () => {
    if (done) return;
    done = true;
    next();
  };
  if (!head.body) {
    play(new Audio(voiceUrl(head.id, location.href)), advance);
    return;
  }
  // 履歴・デモ: 本文を POST して WAV を受け取る (サーバは未合成の部品を飛ばす)
  fetch(announceUrl(location.href), { method: "POST", headers: { "Content-Type": "application/json" }, body: head.body })
    .then((res) => (res.ok ? res.blob() : Promise.reject(new Error(String(res.status)))))
    .then((blob) => {
      const url = URL.createObjectURL(blob);
      play(new Audio(url), () => {
        URL.revokeObjectURL(url);
        advance();
      });
    })
    .catch(advance); // 404・通信エラーは黙って次へ
}

function play(audio: HTMLAudioElement, advance: () => void): void {
  audio.addEventListener("ended", advance);
  audio.addEventListener("error", advance); // 404 などは黙って次へ
  audio.play().catch(advance); // 自動再生の制限なども黙って次へ
}

/** 読み上げを待ち行列に入れる (鳴っていなければ始める) */
export function enqueueVoice(id: string, group: string, priority = false): void {
  queue = enqueue(queue, { id, group, priority });
  if (!playing) next();
}

/** 履歴の再生・デモの報を、本文つきで待ち行列に入れる */
export function enqueueAnnounce(e: EqEvent, groupEvents: readonly EqEvent[], group: string): void {
  queue = enqueue(queue, { id: e.id, group, priority: isPriorityTsunami(e), body: announceBody(e, groupEvents) });
  if (!playing) next();
}

/** 津波予報は報ごとに別のまとまりになるので、続報の比較用に全部の津波予報を発表時刻の順で集め直す */
export function tsunamiHistory(groups: readonly { events: readonly EqEvent[] }[]): EqEvent[] {
  return groups
    .flatMap((g) => g.events.filter((e) => e.kind === "tsunami"))
    .sort((a, b) => ((a as { issued_at: string }).issued_at < (b as { issued_at: string }).issued_at ? -1 : 1));
}
