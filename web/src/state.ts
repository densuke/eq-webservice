// 画面全体で共有する状態と、よく使う定数・関数 (DOM には触らない。テストから読み込めるように)。
// モジュールをまたいで書き換える状態は app にまとめる (import した変数は書き換えられないため)。

import type { Connection } from "./connection.ts";
import type { ScenarioSummary } from "./demo.ts";
import type { Station } from "./detail.ts";
import { GroupStore } from "./groups.ts";
import { loadSettings } from "./personal.ts";
import type { EqEvent, TsunamiEvent } from "./types.ts";

/** 発生からこの秒数を過ぎたら P波・S波の表示を止める */
export const WAVE_MAX_SEC = 180;
/** EEW 警報バナーを出し続ける時間 */
export const EEW_BANNER_MS = 3 * 60_000;
/** 最後の情報からこの時間がたち、揺れも描き終えたら、カメラは日本全体に戻す (選んだ地震と津波予報は除く) */
export const CAMERA_MS = 3 * 60_000;
/** 履歴を選んだときの P波・S波の再生速度 */
export const REPLAY_SPEED = 3;

/** 表示するデータ一式。デモモードでは実際のデータと入れ替える (実際のデータは裏で受け続ける) */
export interface World {
  store: GroupStore;
  /** 受け取った最新の津波予報 (解除を含む)。一覧の整理で消えないよう別に持つ */
  tsunami: TsunamiEvent | null;
}
export const liveWorld: World = { store: new GroupStore(), tsunami: null };

/** デモモードの状態 */
export interface DemoState {
  scenarios: ScenarioSummary[];
  running: string | null;
  timers: number[];
  run: number;
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
};

/** main.ts にある処理。ほかのモジュールからはこれを通して呼ぶ (循環参照を避けるため) */
export const hooks = {
  renderAll: (): void => {},
  onEvents: (_events: EqEvent[], _live: boolean, _target?: World): void => {},
};

/** サーバの時刻 (接続前はブラウザの時刻) */
export function now(): number {
  return app.conn ? app.conn.now() : Date.now();
}
