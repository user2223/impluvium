import type { FetchError } from "ofetch";
import type { NotificationModel } from "~/shared/api/types";
import { sessionContext } from "./session-context.ts";
import { WebSocketHandler, type ConnectionStatus } from "./ws.ts";

export const useNotifications = () =>
  useState<NotificationModel[]>("notifications", () => []);
export const useNotificationConnection = () =>
  useState<ConnectionStatus>("notification-connection", () => "closed");

export function startNotificationConnection() {
  const notifications = useNotifications();
  const status = useNotificationConnection();
  let revision = 0;
  let stopped = true;
  let unauthorized = false;
  let attempt = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let request: AbortController | undefined;
  let live = new Map<string, NotificationModel>();
  const merge = (list: NotificationModel[]) => {
    notifications.value = [
      ...new Map(list.map((item) => [item.id, item])).values(),
    ];
  };
  const stopRestore = () => {
    revision++;
    stopped = true;
    clearTimeout(timer);
    timer = undefined;
    request?.abort();
    request = undefined;
  };
  const current = (generation: number, epoch: number) =>
    !stopped &&
    revision === epoch &&
    generation === sessionContext().generation &&
    !!sessionContext().session;

  async function restore(
    generation: number,
    epoch: number,
    liveUpdates = true,
  ) {
    if (!current(generation, epoch)) return;
    const controller = new AbortController();
    request = controller;
    try {
      const list = await $apiFetch("GET /api/v1/notifications", {
        signal: controller.signal,
        timeout: 15000,
        retry: liveUpdates ? undefined : false,
      });
      if (!current(generation, epoch)) return;
      if (
        !Array.isArray(list) ||
        list.some((item) => !item || typeof item.id !== "string")
      ) {
        throw new Error("The server returned an invalid notification list");
      }
      merge([...live.values(), ...list.filter((item) => !live.has(item.id))]);
      attempt = 0;
      if (liveUpdates) status.value = "connected";
    } catch (error) {
      if (!current(generation, epoch)) return;
      if (!liveUpdates) return;
      const code = (error as FetchError)?.status;
      if (code === 401 || code === 403) {
        unauthorized = true;
        stopped = true;
        status.value = "unauthenticated";
        return;
      }
      status.value = "interrupted";
      // A healthy socket does not reopen after an HTTP recovery failure. Retry
      // the snapshot independently, with bounded requests and backoff.
      const delay = Math.min(1000 * 2 ** attempt++, 30000);
      timer = setTimeout(() => {
        timer = undefined;
        void restore(generation, epoch);
      }, delay);
    } finally {
      if (request === controller) request = undefined;
    }
  }

  const ws = new WebSocketHandler(
    "/api/v1/notifications/ws",
    () => true,
    (value) => {
      status.value = value;
      if (value !== "connected") stopRestore();
      if (value === "unavailable") {
        // Load once without live updates; only a new session can try again.
        stopped = false;
        void restore(sessionContext().generation, revision, false);
      }
    },
    () => {
      stopRestore();
      unauthorized = false;
      live.clear();
      notifications.value = [];
    },
  );
  ws.listen((message) => {
    try {
      const item = JSON.parse(message) as NotificationModel;
      if (!item || typeof item.id !== "string") return;
      live.set(item.id, item);
      merge([item, ...notifications.value.filter((old) => old.id !== item.id)]);
    } catch {
      // Ignore malformed frames without changing the current list.
    }
  });
  ws.onOpen(() => {
    stopRestore();
    if (unauthorized) {
      status.value = "unauthenticated";
      return;
    }
    stopped = false;
    attempt = 0;
    live = new Map();
    void restore(sessionContext().generation, revision);
  });
  return () => ws.dispose();
}
