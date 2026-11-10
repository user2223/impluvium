<template>
  <div ref="currentComponent">
    <VueCarousel :items-to-show="singlePage" :items-to-scroll="singlePage">
      <VueSlide
        v-for="(game, gameIdx) in games"
        :key="gameIdx"
        class="justify-start"
      >
        <GamePanel
          :game="game"
          :href="game ? `/store/${game.id}` : undefined"
          :show-title-description="showGamePanelTextDecoration"
        />
      </VueSlide>

      <template #addons>
        <VueNavigation />
      </template>
    </VueCarousel>
  </div>
</template>

<script setup lang="ts">
import type { ApiResponse } from "~/shared/api/types";

type CarouselGame = ApiResponse<"GET /api/v1/store/featured">[number];

const props = defineProps<{
  items: Array<CarouselGame>;
  min?: number;
  width?: number;
}>();

const {
  store: { showGamePanelTextDecoration },
} = await $apiFetch("GET /api/v1/settings");

const currentComponent = ref<HTMLDivElement>();

const min = computed(() => Math.max(props.min ?? 8, props.items.length));
const games: Ref<Array<CarouselGame | undefined>> = computed(() =>
  Array(min.value)
    .fill(0)
    .map((_, i) => props.items[i]),
);

const singlePage = ref(2);
const sizeOfCard = 192 + 10;

const handleResize = () => {
  singlePage.value =
    (props.width ??
      currentComponent.value?.parentElement?.clientWidth ??
      window.innerWidth) / sizeOfCard;
};

onMounted(() => {
  handleResize();
  window.addEventListener("resize", handleResize);
});

onBeforeUnmount(() => {
  window.removeEventListener("resize", handleResize);
});
</script>
