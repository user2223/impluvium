/** Register language attributes independently of layouts and API availability. */
export function useLocaleHtmlAttributes() {
  // The locale plugin also calls this outside component setup.
  const { $i18n } = useNuxtApp();

  return useHead(() => {
    const locale = unref($i18n.localeProperties);

    return {
      htmlAttrs: {
        lang: locale.language || locale.code,
        // The app uses i18n's default ltr direction.
        dir: locale.dir === "rtl" ? "rtl" : "ltr",
      },
    };
  });
}
