// 画面全体で共有する状態と、よく使う定数・関数 (DOM には触らない。テストから読み込めるように)。
// モジュールをまたいで書き換える状態は app にまとめる (import した変数は書き換えられないため)。

import type { Connection } from "./connection.ts";
import type { Plan, ScenarioSummary } from "./demo.ts";
import type { Station } from "./detail.ts";
import type { Hindsight } from "./history.ts";
import { GroupStore } from "./groups.ts";
import { loadSettings } from "./personal.ts";
import type { EqEvent, TsunamiEvent, UserquakeEvent } from "./types.ts";
import type { Warnings } from "./warnings.ts";

export { WAVE_MAX_SEC } from "./waves.ts";
/** EEW 警報バナーを出し続ける時間 */
export const EEW_BANNER_MS = 3 * 60_000;
/** 履歴を選んだときの P波・S波の再生速度 */
export const REPLAY_SPEED = 3;

/** 表示するデータ一式。デモモードでは実際のデータと入れ替える (実際のデータは裏で受け続ける) */
export interface World {
  store: GroupStore;
  /** 受け取った最新の津波予報 (解除を含む)。一覧の整理で消えないよう別に持つ */
  tsunami: TsunamiEvent | null;
  /** 最新の地震感知情報 (利用者の「揺れた」報告の集計) */
  userquake: UserquakeEvent | null;
}
export const liveWorld: World = { store: new GroupStore(), tsunami: null, userquake: null };

/** デモモードの状態 */
export interface DemoState {
  scenarios: ScenarioSummary[];
  /** 読み込んだ場面 (まだなら null) */
  running: string | null;
  plan: Plan | null;
  /** 場面を再生している世界と、そこへ流し終えた情報の数 */
  world: World | null;
  applied: number;
  run: number;
  /** 履歴の再生 (過去の地震を当時の時刻で流す。場面の一覧は出さず、終わったらライブに戻る) */
  history: boolean;
  /** 時計を飛ばす再生位置の区間 [from, to) (履歴の再生だけ。デモは空) と、「早送り」を出し続ける時刻 (performance.now) */
  skips: { from: number; to: number }[];
  ffUntil: number;
  /** 履歴の再生で、後の報で分かった震源 (再生の始まりから薄く出す。本物の震源が届いたら消す)。デモは null */
  hindsight: Hindsight | null;
  /** デモ専用の時計: anchor (performance.now) の時点で再生位置 pos。speed 倍で進む */
  clock: { pos: number; anchor: number; speed: number; paused: boolean };
}

/** デモの再生位置 (ミリ秒) */
export function demoPos(d: DemoState): number {
  const c = d.clock;
  return c.paused ? c.pos : c.pos + (performance.now() - c.anchor) * c.speed;
}

export const app = {
  /** 表示しているデータ (ふだんは liveWorld、デモ中はデモのデータ) */
  world: liveWorld,
  /** 選んでいる地震 (グループのキー)。null は「最新に自動追従」 */
  selectedKey: null as string | null,
  /** 地震を選んだ時刻 (再生の起点) */
  selectedAt: 0,
  /** 直近の地震の一時的な番号 (グループのキー → 番号) */
  numbers: new Map<string, number>(),
  conn: null as Connection | null,
  /** 震度観測点の位置と属する細分区域 (観測点名 → 位置) */
  stations: new Map<string, Station>(),
  /** P2P地震情報の地域コード -> [地域名, 緯度, 経度] (地震感知情報の位置) */
  userquakeAreas: new Map<number, [string, number, number]>(),
  /** 発表中の気象警報・注意報 (サーバがまとめたもの。未取得なら null) */
  warnings: null as Warnings | null,
  /** デモモードの状態。null ならデモモードではない */
  demo: null as DemoState | null,
  /** 利用者の設定 (端末の中に保存) */
  settings: loadSettings(),
  /** 利用者が観測点の一覧を自分で開閉した地震 (グループのキー → 開いているか) */
  listOpen: new Map<string, boolean>(),
  /** 巡回を始めた時刻 (巡回していなければ null) と、今見せている地震 */
  tourStart: null as number | null,
  tourKey: null as string | null,
  /** 新しく届いた地震をしばらく優先して見せる (巡回より先) */
  tourHold: null as { key: string; until: number } | null,
  /** 「警報・注意報」ボタンで平時に戻した時刻。これ以前に届いた地震の情報は表示を終えたものとする */
  calmSince: 0,
};

/** main.ts にある処理。ほかのモジュールからはこれを通して呼ぶ (循環参照を避けるため) */
export const hooks = {
  renderAll: (): void => {},
  onEvents: (_events: EqEvent[], _live: boolean, _target?: World): void => {},
};

/** サーバの時刻 (接続前はブラウザの時刻) */
export function now(): number {
  // デモの場面を再生している間は、デモの時計 (記録の場面は当時の日時)
  if (app.demo?.plan) return app.demo.plan.toReal(demoPos(app.demo));
  return app.conn ? app.conn.now() : Date.now();
}
