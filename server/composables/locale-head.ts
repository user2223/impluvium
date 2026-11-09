/** Register language attributes independently of layouts and API availability. */
export function useLocaleHtmlAttributes() {
  const localeHead = useLocaleHead({ seo: false });

  return useHead(() => ({
    htmlAttrs: {
      lang: localeHead.value.htmlAttrs?.lang ?? "en-us",
      dir: localeHead.value.htmlAttrs?.dir === "rtl" ? "rtl" : "ltr",
    },
  }));
}
