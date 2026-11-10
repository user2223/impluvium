import type { TaskLog } from "~/shared/api/types";

const labelNumberMap = {
  100: "silent",
  60: "fatal",
  50: "error",
  40: "warn",
  30: "info",
  20: "debug",
  10: "trace",
  0: "off",
};

export function parseTaskLog(logStr?: string | undefined): TaskLog {
  if (!logStr) return { msg: "", time: "", level: "" };
  const log = JSON.parse(logStr);

  if (typeof log.level === "number") {
    log.level = labelNumberMap[
      log.level as keyof typeof labelNumberMap
    ] as string;
  }

  return log;
}
