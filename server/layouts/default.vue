<template>
  <div
    v-if="!clientRequest"
    class="flex flex-col w-full min-h-screen bg-zinc-900"
  >
    <LazyUserHeader class="z-50" />
    <div class="grow flex">
      <NuxtPage />
    </div>
    <LazyUserFooter class="z-50" />
  </div>
  <div v-else class="flex flex-col w-full min-h-screen bg-zinc-900">
    <NuxtPage />
    <LazyUserHeaderStoreNav />
  </div>
</template>

<script setup lang="ts">
const clientRequest = isClientRequest();

const { t } = useI18n();

useHead({
  titleTemplate(title) {
    return title ? t("titleTemplate", [title]) : t("title");
  },
});
const { mLogoObjectId } = await $apiFetch("GET /api/v1");
const favicon = mLogoObjectId ? useObject(mLogoObjectId) : "/favicon.ico";
useFavicon(favicon, { rel: "icon" });
</script>
