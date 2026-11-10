import type { TaskMessage } from "~/shared/api/types";
import { WebSocketHandler, type ConnectionStatus } from "./ws.ts";

const taskStates = new Map<string, Ref<TaskMessage | undefined>>();
const observers = new Map<string, number>();
const subscriptions = new Set<string>();
let channel: WebSocketHandler | undefined;
export const useTaskReady = () => useState("taskready", () => false);
export const useTaskConnection = () =>
  useState<ConnectionStatus>("task-connection", () => "closed");

export function startTaskConnection() {
  const ready = useTaskReady();
  const status = useTaskConnection();
  const awaitingSnapshot = new Set<string>();
  const ws = new WebSocketHandler(
    "/api/v1/task",
    (session) => session.admin,
    (value) => {
      status.value = value;
      if (value !== "connected") ready.value = false;
    },
    () => {
      for (const state of taskStates.values()) state.value = undefined;
      taskStates.clear();
      observers.clear();
      subscriptions.clear();
      awaitingSnapshot.clear();
      ready.value = false;
    },
  );
  channel = ws;
  ws.onOpen(() => {
    for (const id of subscriptions) {
      awaitingSnapshot.add(id);
      taskStates.get(id)!.value = undefined;
      ws.send(`connect/${id}`);
    }
  });
  ws.listen((message) => {
    if (message === "connect") {
      ready.value = true;
      status.value = "connected";
      return;
    }
    try {
      const msg = JSON.parse(message) as TaskMessage;
      if (!msg || typeof msg.id !== "string" || !Array.isArray(msg.log)) return;
      const state = taskStates.get(msg.id);
      if (!state || !subscriptions.has(msg.id)) return;
      if (awaitingSnapshot.delete(msg.id) || !state.value || msg.reset) {
        state.value = msg;
      } else {
        const log = [...state.value.log, ...msg.log];
        state.value = { ...msg, log };
      }
    } catch {
      const [action, id, title, description] = message.split("/");
      const state = taskStates.get(id);
      if (action === "disconnect") {
        // The server sends this after the final snapshot. Keep that result
        // visible and owned by the session, but stop resubscribing this task.
        subscriptions.delete(id);
        awaitingSnapshot.delete(id);
      } else if (action === "error" && state) {
        state.value = {
          id,
          name: "Unknown task",
          success: false,
          progress: 0,
          error: { title, description },
          log: [],
          actions: [],
        };
      }
    }
  });
  return () => {
    ws.dispose();
    if (channel === ws) channel = undefined;
  };
}

export const useTask = (taskId: string): Ref<TaskMessage | undefined> => {
  let state = taskStates.get(taskId);
  if (!state) {
    state = ref(undefined);
    taskStates.set(taskId, state);
    subscriptions.add(taskId);
    channel?.send(`connect/${taskId}`);
  }
  observers.set(taskId, (observers.get(taskId) ?? 0) + 1);
  const observedState = state;
  if (getCurrentScope()) {
    onScopeDispose(() => {
      if (taskStates.get(taskId) !== observedState) return;
      const remaining = (observers.get(taskId) ?? 1) - 1;
      if (remaining > 0) observers.set(taskId, remaining);
      else {
        observers.delete(taskId);
        taskStates.delete(taskId);
        subscriptions.delete(taskId);
      }
    });
  }
  return state;
};
