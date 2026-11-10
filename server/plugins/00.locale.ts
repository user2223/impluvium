import {
  LEGACY_UI_LOCALE_COOKIE,
  UI_LOCALE_COOKIE,
  decodeLocaleCookie,
  legacyUiLocale,
  malformedLocaleCookies,
} from "~/utils/locale-cookie";

// Nuxt decodes cookies without catching errors, and i18n reads its own cookie
// through that path, so a malformed value has to be gone before i18n starts.
function expireMalformedLocaleCookies() {
  const expired = malformedLocaleCookies(document.cookie).map(
    (name) => `${name}=; Max-Age=0; Path=/; SameSite=Lax`,
  );
  if (expired.length === 0) return;
  for (const cookie of expired) document.cookie = cookie;
}

export default defineNuxtPlugin({
  name: "ui-locale",
  // Register the app:created hook before the router's pre-plugin hook.
  order: -30,
  setup(nuxtApp) {
    expireMalformedLocaleCookies();

    // Capture the browser preference before i18n can create its default cookie.
    const current = useCookie<unknown>(UI_LOCALE_COOKIE, {
      readonly: true,
      decode: decodeLocaleCookie,
    }).value;
    const legacy = useCookie<unknown>(LEGACY_UI_LOCALE_COOKIE, {
      path: "/",
      sameSite: "lax",
      decode: decodeLocaleCookie,
    });
    const previous = legacy.value;
    if (previous !== undefined && previous !== null) legacy.value = null;

    // All plugins, including i18n's initial detection, finish before this hook.
    // Apply the imported preference before app setup starts any API requests.
    nuxtApp.hook("app:created", async () => {
      const locale = legacyUiLocale(
        current,
        previous,
        unref(nuxtApp.$i18n.localeCodes),
      );
      if (locale !== undefined) {
        await nuxtApp.runWithContext(() => nuxtApp.$i18n.setLocale(locale));
        nuxtApp.$i18n.setLocaleCookie(locale);
      }
      nuxtApp.runWithContext(() => useLocaleHtmlAttributes());
    });
  },
});
