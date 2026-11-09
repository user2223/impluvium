<template>
  <component :is="tag" :key="renderKey" ref="container">
    <slot />
  </component>
</template>

<script setup lang="ts" generic="T">
import Sortable from "sortablejs";

const props = withDefaults(
  defineProps<{ tag?: "div" | "tbody"; handle?: string }>(),
  { tag: "div", handle: undefined },
);
const model = defineModel<T[]>({ required: true });
const emit = defineEmits<{ change: [] }>();
const container = ref<HTMLElement>();
const renderKey = ref(0);
let sortable: Sortable | undefined;
type DragSnapshot = {
  order: T[];
  startIndex: number;
  nextSibling: ChildNode | null;
};
let drag: DragSnapshot | undefined;

function destroySortable() {
  drag = undefined;
  const instance = sortable;
  sortable = undefined;
  instance?.destroy();
}

watch(
  () => model.value.slice(),
  () => {
    if (!drag) return;
    // Cancel even before onStart, and rebuild the DOM from the current model.
    destroySortable();
    renderKey.value++;
  },
  { flush: "sync" },
);

// The slot renders one keyed element per model entry. Vue owns their order.
watch(
  container,
  (element) => {
    destroySortable();
    if (!element) return;
    sortable = new Sortable(element, {
      handle: props.handle,
      draggable: ">*",
      onChoose({ item, from }) {
        // Capture the order and its DOM index together, before dragging starts.
        drag = {
          order: model.value.slice(),
          startIndex: Array.from(from.children).indexOf(item),
          nextSibling: item.nextSibling,
        };
      },
      onUnchoose() {
        const snapshot = drag;
        // onEnd follows synchronously only when a drag actually started.
        queueMicrotask(() => {
          if (drag === snapshot) drag = undefined;
        });
      },
      onEnd({ item, from, newDraggableIndex }) {
        const snapshot = drag;
        drag = undefined;
        if (!snapshot || item.parentNode !== from) return;
        // Undo Sortable's DOM move before Vue patches the reordered model.
        from.insertBefore(item, snapshot.nextSibling);
        if (
          typeof newDraggableIndex !== "number" ||
          snapshot.startIndex === newDraggableIndex ||
          snapshot.startIndex < 0 ||
          newDraggableIndex < 0 ||
          snapshot.startIndex >= snapshot.order.length ||
          newDraggableIndex >= snapshot.order.length
        ) {
          return;
        }

        const reordered = snapshot.order.slice();
        const [moved] = reordered.splice(snapshot.startIndex, 1);
        reordered.splice(newDraggableIndex, 0, moved!);
        model.value = reordered;
        emit("change");
      },
    });
  },
  { flush: "post" },
);

onBeforeUnmount(destroySortable);
</script>
