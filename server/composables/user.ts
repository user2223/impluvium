import type { UserModel } from "~/shared/api/types";
import {
  advanceSessionContext,
  confirmSessionContext,
  sessionContext,
} from "./session-context";

// undefined = haven't check
// null = check, no user
// {} = check, user

export const useUser = () => useState<UserModel | undefined | null>(undefined);
async function loadUser(onConfirmed?: (generation: number) => void) {
  const user = useUser();
  if (user.value === null) return;

  const generation = sessionContext().generation;
  const value = await $apiFetch("GET /api/v1/user", { timeout: 15000 });
  if (generation !== sessionContext().generation) return;
  confirmSessionContext(value);
  onConfirmed?.(sessionContext().generation);
  user.value = value;
}

export const updateUser = () => loadUser();

let initialization: { generation: number; promise: Promise<void> } | undefined;

/** Share the startup decision, including failure, across the router and app. */
export function initializeUser(): Promise<void> {
  const generation = sessionContext().generation;
  if (initialization?.generation === generation) return initialization.promise;
  const promise = loadUser((confirmedGeneration) => {
    // Confirming the initial user can itself advance the session generation.
    if (initialization?.promise === promise) {
      initialization.generation = confirmedGeneration;
    }
  });
  initialization = { generation, promise };
  return promise;
}

export async function completeSignin() {
  const route = useRoute();
  const router = useRouter();

  const user = useUser();
  advanceSessionContext();
  user.value = undefined;
  const generation = sessionContext().generation;
  const value = await $apiFetch("GET /api/v1/user", { timeout: 15000 });
  if (generation !== sessionContext().generation) return;
  confirmSessionContext(value);
  user.value = value;
  await router.push(route.query.redirect?.toString() ?? "/");
}
