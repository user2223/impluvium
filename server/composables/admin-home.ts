import type { SystemData } from "~/shared/api/types";
import { WebSocketHandler, type ConnectionStatus } from "./ws.ts";

const emptySystemData = (): SystemData => ({
  totalRam: 0,
  freeRam: 0,
  cpuLoad: 0,
  cpuCores: 0,
});
export const useSystemData = () =>
  useState<SystemData>("system-data", emptySystemData);
export const useSystemConnection = () =>
  useState<ConnectionStatus>("system-connection", () => "closed");

export function startSystemConnection() {
  const data = useSystemData();
  const status = useSystemConnection();
  const ws = new WebSocketHandler(
    "/api/v1/admin/system-data/ws",
    (session) => session.admin,
    (value) => {
      status.value = value;
    },
    () => {
      data.value = emptySystemData();
    },
  );
  ws.listen((message) => {
    try {
      const next = JSON.parse(message) as SystemData | null;
      if (
        !next ||
        ![next.totalRam, next.freeRam, next.cpuLoad, next.cpuCores].every(
          (value) => typeof value === "number" && Number.isFinite(value),
        )
      ) {
        return;
      }
      data.value = next;
      status.value = "connected";
    } catch {
      // Keep interrupted data stale until a valid frame arrives.
    }
  });
  return () => ws.dispose();
}
