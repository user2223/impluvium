import { IconsLinuxLogo, IconsWindowsLogo, IconsMacLogo } from "#components";
import { Platform } from "~/shared/api/generated/enums";

export const PLATFORM_ICONS = {
  [Platform.Linux]: IconsLinuxLogo,
  [Platform.Windows]: IconsWindowsLogo,
  [Platform.macOS]: IconsMacLogo,
};
