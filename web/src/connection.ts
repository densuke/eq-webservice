// サーバとの WebSocket 接続。切断時は自動で再接続し、サーバ時刻との差を保持する。

import type { EqEvent, ServerMessage } from "./types.ts";

export type Status = "connecting" | "open" | "closed";

interface Handlers {
  onSnapshot(events: EqEvent[]): void;
  onEvent(event: EqEvent): void;
  onStatus(status: Status): void;
}

export class Connection {
  /** サーバ時刻 − ブラウザ時刻 (ミリ秒) */
  clockOffsetMs = 0;
  private backoff = 1000;
  private ws: WebSocket | null = null;

  constructor(
    private url: string,
    private handlers: Handlers,
  ) {}

  /** ページの場所からの相対パスで ws URL を作る (リバースプロキシのサブパス配下でも動く) */
  static defaultUrl(): string {
    const u = new URL("ws", location.href);
    u.protocol = u.protocol === "https:" ? "wss:" : "ws:";
    return u.toString();
  }

  now(): number {
    return Date.now() + this.clockOffsetMs;
  }

  start(): void {
    this.handlers.onStatus("connecting");
    const ws = new WebSocket(this.url);
    this.ws = ws;
    ws.onopen = () => {
      this.backoff = 1000;
      this.handlers.onStatus("open");
    };
    ws.onmessage = (e) => {
      let msg: ServerMessage;
      try {
        msg = JSON.parse(e.data);
      } catch {
        return;
      }
      this.clockOffsetMs = msg.server_time_ms - Date.now();
      if (msg.type === "hello") this.handlers.onSnapshot(msg.events);
      else if (msg.type === "event") this.handlers.onEvent(msg.event);
    };
    ws.onclose = () => {
      if (this.ws !== ws) return;
      this.handlers.onStatus("closed");
      setTimeout(() => this.start(), this.backoff);
      this.backoff = Math.min(this.backoff * 2, 30_000);
    };
    ws.onerror = () => ws.close();
  }
}
