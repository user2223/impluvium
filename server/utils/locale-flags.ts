// The flags of the language selection are static files of the web application,
// served from /flags/<codepoint>.svg without any sign-in. The build copies
// exactly the files named here from the emoji package (tools/web-spa/build.mjs).
// The selection must never load them through the signed-in emoji route.

const FLAGS = new Map<string, string>([
  // English (US), also the default language
  ["en", "1f1fa-1f1f8"],
  ["en-us", "1f1fa-1f1f8"],
  // English (UK)
  ["en-gb", "1f1ec-1f1e7"],
  // English (Australia)
  ["en-au", "1f1e6-1f1fa"],
  // English (Pirate): pirate flag
  ["en-pirate", "1f3f4-200d-2620-fe0f"],
  ["fr", "1f1eb-1f1f7"],
  ["de", "1f1e9-1f1ea"],
  ["es", "1f1ea-1f1f8"],
  ["it", "1f1ee-1f1f9"],
  ["zh", "1f1e8-1f1f3"],
  ["zh-tw", "1f1f9-1f1fc"],
]);

/** The flag for a language without a flag of its own: a question mark. */
export const UNKNOWN_FLAG = "2753";

/** The code point name of the flag of a language. */
export function localeFlag(locale: string): string {
  return FLAGS.get(locale) ?? UNKNOWN_FLAG;
}

/** The address of the flag of a language. */
export function localeFlagUrl(locale: string): string {
  return `/flags/${localeFlag(locale)}.svg`;
}

/** Every flag the web application ships, without duplicates, sorted. */
export const flagCodepoints: readonly string[] = [
  ...new Set([...FLAGS.values(), UNKNOWN_FLAG]),
].sort();
