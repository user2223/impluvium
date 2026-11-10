import type { ApiResponse } from "~/shared/api/types";

type FullCollection = ApiResponse<"GET /api/v1/collection">[number];
type Library = ApiResponse<"GET /api/v1/collection/default">;

export const useCollections = async () => {
  // @ts-expect-error undefined is used to tell if value has been fetched or not
  const state = useState<FullCollection[]>("collections", () => undefined);
  if (state.value === undefined) {
    state.value = await $apiFetch("GET /api/v1/collection");
  }

  return state;
};

export async function refreshCollection(id: string) {
  const state = useState<FullCollection[]>("collections");
  const collection = await $apiFetch("GET /api/v1/collection/:id", {
    path: { id },
  });
  const index = state.value.findIndex((e) => e.id == id);
  if (index == -1) {
    state.value.push(collection);
    return;
  }
  state.value[index] = collection;
}

export const useLibrary = async () => {
  // @ts-expect-error undefined is used to tell if value has been fetched or not
  const state = useState<Library>("library", () => undefined);
  if (state.value === undefined) {
    await refreshLibrary();
  }

  return state;
};

export async function refreshLibrary() {
  const state = useState<Library>("library");
  state.value = await $apiFetch("GET /api/v1/collection/default");
}
