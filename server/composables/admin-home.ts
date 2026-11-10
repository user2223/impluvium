import type { SystemData } from "~/shared/api/types";

const ws = new WebSocketHandler("/api/v1/admin/system-data/ws");

export const useSystemData = () =>
  useState<SystemData>("system-data", () => ({
    totalRam: 0,
    freeRam: 0,
    cpuLoad: 0,
    cpuCores: 0,
  }));

ws.listen((systemDataString) => {
  const data = JSON.parse(systemDataString) as SystemData;
  const systemData = useSystemData();
  systemData.value = data;
});
