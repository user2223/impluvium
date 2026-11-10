import {
  onSessionContext,
  sessionContext,
  type BrowserSession,
} from "./session-context.ts";

export type ConnectionStatus =
  | "closed"
  | "connecting"
  | "interrupted"
  | "connected"
  | "unavailable"
  | "unauthenticated";

/** Constructing a channel registers its lifecycle; it never opens on import. */
export class WebSocketHandler {
  private ws: WebSocket | undefined;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private attempt = 0;
  private blocked = false;
  private generation = -1;
  private disposeSession: () => void;
  private listeners = new Set<(message: string) => void>();
  private openListeners = new Set<() => void>();
  private route: string;
  private allowed: (session: BrowserSession) => boolean;
  private status: (value: ConnectionStatus) => void;
  private reset: () => void;

  constructor(
    route: string,
    allowed: (session: BrowserSession) => boolean,
    status: (value: ConnectionStatus) => void,
    reset: () => void,
  ) {
    this.route = route;
    this.allowed = allowed;
    this.status = status;
    this.reset = reset;
    this.disposeSession = onSessionContext((session) => {
      const generation = sessionContext().generation;
      if (this.generation !== generation) {
        this.close();
        this.reset();
        this.blocked = false;
        this.attempt = 0;
        this.generation = generation;
      }
      if (
        session &&
        this.allowed(session) &&
        !this.ws &&
        !this.timer &&
        !this.blocked
      ) {
        this.open();
      }
    });
  }

  private close() {
    clearTimeout(this.timer);
    this.timer = undefined;
    const ws = this.ws;
    this.ws = undefined;
    ws?.close();
    this.status("closed");
  }

  private open() {
    if (
      isClientRequest() ||
      (location.protocol !== "http:" && location.protocol !== "https:")
    ) {
      // Embedded documents need a separate live update transport.
      this.blocked = true;
      this.status("unavailable");
      return;
    }
    const generation = this.generation;
    const url =
      (location.protocol === "https:" ? "wss://" : "ws://") +
      location.host +
      this.route;
    let ws: WebSocket;
    try {
      ws = new WebSocket(url);
    } catch {
      this.retry(generation);
      return;
    }
    this.ws = ws;
    const current = () =>
      this.ws === ws && generation === sessionContext().generation;
    // Keep the warning visible while recovering from a failed connection.
    this.status(this.attempt > 0 ? "interrupted" : "connecting");
    ws.onopen = () => {
      if (!current()) return;
      // A channel becomes current only after its consumer restores its state.
      for (const callback of this.openListeners) callback();
    };
    ws.onmessage = (event) => {
      if (!current()) return;
      if (event.data === "unauthenticated") {
        this.blocked = true;
        this.close();
        this.status("unauthenticated");
        return;
      }
      this.attempt = 0;
      for (const callback of this.listeners) callback(String(event.data));
    };
    ws.onclose = (event) => {
      if (!current()) return;
      this.ws = undefined;
      if (event.code === 1008 || event.code === 4401 || event.code === 4403) {
        this.blocked = true;
        this.status("unauthenticated");
        return;
      }
      this.retry(generation);
    };
    ws.onerror = () => {
      if (current()) {
        this.status("interrupted");
        ws.close();
      }
    };
  }

  private retry(generation: number) {
    this.status("interrupted");
    if (this.blocked || !sessionContext().session || this.timer) return;
    const delay = Math.min(1000 * 2 ** this.attempt++, 30000);
    this.timer = setTimeout(() => {
      this.timer = undefined;
      if (
        generation === sessionContext().generation &&
        sessionContext().session
      ) {
        this.open();
      }
    }, delay);
  }

  listen(callback: (message: string) => void) {
    this.listeners.add(callback);
  }
  onOpen(callback: () => void) {
    this.openListeners.add(callback);
  }
  send(message: string) {
    if (this.ws?.readyState === WebSocket.OPEN) this.ws.send(message);
  }
  dispose() {
    this.disposeSession();
    this.close();
    this.reset();
    this.listeners.clear();
    this.openListeners.clear();
  }
}
