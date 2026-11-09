export function formatBytes(bytes: number): string {
  // TODO: use i18n formatting https://vue-i18n.intlify.dev/guide/essentials/number.html

  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes >= 1024 && bytes < Math.pow(1024, 2)) {
    return `${(bytes / 1024).toFixed(2)} KiB`;
  }
  if (bytes >= Math.pow(1024, 2) && bytes < Math.pow(1024, 3)) {
    return `${(bytes / (1024 * 1024)).toFixed(2)} MiB`;
  }
  if (bytes >= Math.pow(1024, 3) && bytes < Math.pow(1024, 4)) {
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GiB`;
  }
  return `${(bytes / Math.pow(1024, 4)).toFixed(2)} TiB`;
}
