import type { $Fetch, FetchError, FetchOptions } from "ofetch";
import type { ApiFetch, ApiFetchKey, ApiOptions } from "~/shared/api/client";
import { apiRoutes } from "~/shared/api/generated/routes";
import { assertApiFetchOptions, resolveApiPath } from "~/shared/api/request";
import { createBrowserTransport } from "./browser-transport";
import { advanceSessionContext, sessionContext } from "./session-context";

const transport = createBrowserTransport((url, options) =>
  ($fetch as unknown as $Fetch).raw(url, options),
);

export const $apiFetch: ApiFetch = (key, ...args) => {
  const route = apiRoutes[key];
  if (!route) throw new Error(`Unknown API route: ${key}`);
  const options = (args[0] ?? {}) as ApiOptions<ApiFetchKey>;
  assertApiFetchOptions(options);
  const { path, ...fetchOptions } = options;
  const request = resolveApiPath(route.path, path);
  if (
    route.method === "POST" &&
    (request.startsWith("/api/v1/auth/") || request === "/api/v1/setup")
  ) {
    advanceSessionContext();
  }
  return dropTransport(request, {
    ...fetchOptions,
    method: route.method,
    responseType: route.kind.startsWith("binary")
      ? "blob"
      : route.kind.startsWith("text")
        ? "text"
        : "json",
  });
};

async function dropTransport<T>(
  request: string,
  options: FetchOptions & { failTitle?: string } = {},
): Promise<T> {
  const generation = sessionContext().generation;
  const { failTitle, ...fetchOptions } = options;
  try {
    return (await transport(request, fetchOptions)) as T;
  } catch (error) {
    if (
      generation === sessionContext().generation &&
      !fetchOptions.signal?.aborted &&
      !(error instanceof DOMException && error.name === "AbortError") &&
      failTitle
    ) {
      const data: unknown = (error as FetchError)?.data;
      const message =
        data &&
        typeof data === "object" &&
        "message" in data &&
        typeof data.message === "string"
          ? data.message
          : "The request could not be completed. Please try again.";
      createModal(
        ModalType.Notification,
        { title: failTitle, description: message },
        (_, close) => close(),
      );
    }
    throw error;
  }
}

/** This presentation hint never grants API permissions. */
export function isClientRequest() {
  return navigator.userAgent.startsWith("impluvium-desktop/");
}
