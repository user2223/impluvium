<template>
  <NuxtLoadingIndicator color="#2563eb" />
  <div
    v-if="connectionInterrupted"
    role="status"
    class="bg-amber-950 px-4 py-2 text-amber-100"
  >
    {{ connectionWarning }}
  </div>
  <NuxtLayout>
    <NuxtPage />
  </NuxtLayout>
  <ModalStack />
  <div
    v-if="showExternalUrlWarning"
    class="fixed flex flex-row gap-x-2 right-0 bottom-0 m-2 px-2 py-2 z-50 text-right bg-red-700/90 rounded-lg"
  >
    <div class="flex flex-col">
      <span class="text-sm text-zinc-200 font-bold font-display">{{
        $t("errors.externalUrl.title")
      }}</span>
      <span class="text-xs text-red-400">{{
        $t("errors.externalUrl.subtitle")
      }}</span>
    </div>
    <button class="text-red-200" @click="() => hideExternalURL()">
      <XMarkIcon class="size-5" />
    </button>
  </div>
</template>

<script setup lang="ts">
import { XMarkIcon } from "@heroicons/vue/24/outline";
import {
  advanceSessionContext,
  onSessionContext,
  sessionContext,
} from "./composables/session-context";
import type { ApiResponse } from "./shared/api/types";

const user = useUser();
const nuxtApp = useNuxtApp();
const connectionWarning =
  "Live updates are interrupted. Displayed values may be out of date.";
let contextGeneration = sessionContext().generation;
const stopContext = onSessionContext((session) => {
  const generation = sessionContext().generation;
  if (session || generation === contextGeneration) return;
  contextGeneration = generation;
  user.value = undefined;
  nuxtApp.runWithContext(() =>
    clearNuxtState(["collections", "library", "news", "users"]),
  );
});

const stopNotifications = startNotificationConnection();
const stopTasks = startTaskConnection();
const stopSystem = startSystemConnection();
const notificationStatus = useNotificationConnection();
const taskStatus = useTaskConnection();
const systemStatus = useSystemConnection();
const connectionInterrupted = computed(() =>
  [notificationStatus.value, taskStatus.value, systemStatus.value].some(
    (value) =>
      value === "interrupted" ||
      value === "connecting" ||
      value === "unauthenticated",
  ),
);

// Full-document auth routes bypass the client router. End the context before
// their navigation, including clicks inside the embedded desktop store.
const beforeNavigation = (event: MouseEvent) => {
  const anchor = (event.target as Element | null)?.closest("a[href]");
  if (!anchor) return;
  const url = new URL(anchor.getAttribute("href")!, location.href);
  if (
    url.origin === location.origin &&
    (url.pathname === "/auth/signout" || url.pathname === "/auth/oidc")
  ) {
    advanceSessionContext();
    user.value = undefined;
  }
};
const restoreDocument = (event: PageTransitionEvent) => {
  // A back/forward cache entry has already ended its session context. Rebuild
  // its component-owned data rather than resuming those old references.
  if (event.persisted) location.reload();
};
document.addEventListener("click", beforeNavigation, true);
window.addEventListener("pagehide", advanceSessionContext);
window.addEventListener("pageshow", restoreDocument);
onScopeDispose(() => {
  document.removeEventListener("click", beforeNavigation, true);
  window.removeEventListener("pagehide", advanceSessionContext);
  window.removeEventListener("pageshow", restoreDocument);
  stopContext();
  stopNotifications();
  stopTasks();
  stopSystem();
});

let apiDetails: ApiResponse<"GET /api/v1">;
try {
  await initializeUser();
  const details = await $apiFetch("GET /api/v1", { timeout: 15000 });
  if (!details || typeof details.external !== "string") {
    throw new Error("The server returned invalid application information");
  }
  apiDetails = details;
} catch (error) {
  document.getElementById("impluvium-loading")?.remove();
  throw createError({
    statusCode: 503,
    fatal: true,
    statusMessage: "Unable to initialize the application. Please try again.",
    cause: error,
  });
}
onMounted(() => document.getElementById("impluvium-loading")?.remove());

const clientMode = isClientRequest();

const showExternalUrlWarning = ref(false);
function checkExternalUrl() {
  if (clientMode) return;
  const realOrigin = window.location.origin.trim();
  const chosenOrigin = apiDetails.external.trim();
  const ignore = window.localStorage.getItem("ignoreExternalUrl");
  if (ignore && ignore == "true") return;
  showExternalUrlWarning.value = !(realOrigin == chosenOrigin);
}

function hideExternalURL() {
  window.localStorage.setItem("ignoreExternalUrl", "true");
  showExternalUrlWarning.value = false;
}

if (user.value?.admin) {
  onMounted(() => {
    checkExternalUrl();
  });
}
</script>
