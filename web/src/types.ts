// サーバ (crates/eq-core/src/model.rs) の JSON 形式に対応する型。

export type Scale = number; // -1 不明, 10=震度1 ... 45=5弱, 50=5強, 55=6弱, 60=6強, 70=7

export interface Hypocenter {
  name: string;
  latitude: number | null;
  longitude: number | null;
  depth_km: number | null;
  magnitude: number | null;
}

export interface PrefScale {
  pref: string;
  scale: Scale;
}

export interface ObservationPoint {
  pref: string;
  addr: string;
  is_area: boolean;
  scale: Scale;
  /** 過去の記録のデモだけ: 今は無い観測点の位置と細分区域 */
  station?: { lat: number; lon: number; area: string };
}

interface EventBase {
  id: string;
  source: string;
  received_at_ms: number;
}

export interface QuakeEvent extends EventBase {
  kind: "quake";
  info_type: "scale_prompt" | "destination" | "scale_and_destination" | "detail_scale" | "foreign" | "other";
  origin_time: string;
  origin_time_ms: number | null;
  issued_at: string;
  hypocenter: Hypocenter | null;
  max_scale: Scale;
  domestic_tsunami: string;
  points: ObservationPoint[];
  pref_max: PrefScale[];
  comment: string;
}

export interface EewArea {
  pref: string;
  name: string;
  scale_from: Scale;
  scale_to: Scale | null;
  arrival_time: string | null;
  arrived: boolean;
}

export interface EewEvent extends EventBase {
  kind: "eew";
  event_id: string;
  serial: string;
  cancelled: boolean;
  test: boolean;
  /** 警報 (予測震度5弱以上)。false は予報 */
  warning: boolean;
  issued_at: string;
  origin_time: string | null;
  origin_time_ms: number | null;
  hypocenter: Hypocenter | null;
  areas: EewArea[];
  pref_max: PrefScale[];
  max_scale: Scale;
}

export interface EewDetectionEvent extends EventBase {
  kind: "eew_detection";
  detection_type: string;
}

export interface TsunamiArea {
  name: string;
  grade: "unknown" | "watch" | "warning" | "major_warning";
  immediate: boolean;
  first_height: string | null;
  max_height: string | null;
}

export interface TsunamiEvent extends EventBase {
  kind: "tsunami";
  cancelled: boolean;
  issued_at: string;
  areas: TsunamiArea[];
}

export type EqEvent = QuakeEvent | EewEvent | EewDetectionEvent | TsunamiEvent;

export type ServerMessage =
  | { type: "hello"; server_time_ms: number; events: EqEvent[] }
  | { type: "event"; server_time_ms: number; event: EqEvent };
