export const UI_LOCALE_COOKIE = "impluvium_i18n_redirected";
export const LEGACY_UI_LOCALE_COOKIE = "drop_i18n_redirected";

/** A malformed cookie is treated as absent and must never stop the app from starting. */
export function decodeLocaleCookie(value: string): string | undefined {
  try {
    return decodeURIComponent(value);
  } catch {
    return undefined;
  }
}

/** The browser locale cookies whose values cannot be decoded. */
export function malformedLocaleCookies(cookies: string): string[] {
  const malformed = new Set<string>();
  for (const pair of cookies.split(";")) {
    const separator = pair.indexOf("=");
    if (separator === -1) continue;
    const name = pair.slice(0, separator).trim();
    if (name !== UI_LOCALE_COOKIE && name !== LEGACY_UI_LOCALE_COOKIE) continue;
    if (decodeLocaleCookie(pair.slice(separator + 1).trim()) === undefined) {
      malformed.add(name);
    }
  }
  return [...malformed];
}

/** Only an absent current cookie permits importing a supported legacy value. */
export function legacyUiLocale<Locale extends string>(
  current: unknown,
  legacy: unknown,
  locales: readonly Locale[],
): Locale | undefined {
  if (current !== undefined && current !== null) return undefined;
  return locales.find((locale) => locale === legacy);
}
