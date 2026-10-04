export interface Item {
  id: string;
  group: string;
}

export const MAX_VOICES = 4;

/** 待ち行列に足した新しい配列を返す。同じ group が待っていればその位置で差し替え、溢れたら古い方から捨てる */
export function enqueue(queue: readonly Item[], item: Item): Item[] {
  const i = queue.findIndex((q) => q.group === item.group);
  const next = i >= 0 ? queue.map((q, j) => (j === i ? item : q)) : [...queue, item];
  return next.slice(Math.max(0, next.length - MAX_VOICES));
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
  const audio = new Audio(voiceUrl(head.id, location.href));
  // 404 では error と play() の失敗が両方起きるので、次へ進むのは 1 回だけにする
  let done = false;
  const advance = () => {
    if (done) return;
    done = true;
    next();
  };
  audio.addEventListener("ended", advance);
  audio.addEventListener("error", advance); // 404 などは黙って次へ
  audio.play().catch(advance); // 自動再生の制限なども黙って次へ
}

/** 読み上げを待ち行列に入れる (鳴っていなければ始める) */
export function enqueueVoice(id: string, group: string): void {
  queue = enqueue(queue, { id, group });
  if (!playing) next();
}
