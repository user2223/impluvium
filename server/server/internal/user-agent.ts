import { systemConfig } from "./config/sys-conf";

export const USER_AGENT_TOKEN = "impluvium-server";
export const USER_AGENT_REPOSITORY = "https://github.com/user2223/impluvium";

// Same spelling as Rust's std::env::consts, so every component reports one form.
const OS_NAMES: Record<string, string> = { win32: "windows" };
const ARCH_NAMES: Record<string, string> = { x64: "x86_64", arm64: "aarch64" };

function platform() {
  const os = OS_NAMES[process.platform] ?? process.platform;
  const arch = ARCH_NAMES[process.arch] ?? process.arch;
  return `${os}; ${arch}`;
}

/** User agent for requests to our own components. */
export function serverUserAgent() {
  return `${USER_AGENT_TOKEN}/${systemConfig.getDropVersion()} (${platform()})`;
}

/** User agent for third-party services; names the project so operators can reach us. */
export function thirdPartyUserAgent() {
  return `${USER_AGENT_TOKEN}/${systemConfig.getDropVersion()} (${platform()}; +${USER_AGENT_REPOSITORY})`;
}
