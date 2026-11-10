import type { FetchOptions } from "ofetch";
import { requestFingerprint } from "../shared/api/request.ts";
import { sessionContext } from "./session-context.ts";

interface BrowserResponse {
  status: number;
  _data?: unknown;
}
type Dispatch = (
  url: string,
  options: FetchOptions,
) => Promise<BrowserResponse>;
interface PendingRequest {
  response: Promise<unknown>;
  controller: AbortController;
  readers: number;
  settled: boolean;
}

function stableKey(value: unknown): string {
  return JSON.stringify(value, (_, item) =>
    item && typeof item === "object" && !Array.isArray(item)
      ? Object.fromEntries(
          Object.entries(item).sort(([a], [b]) => a.localeCompare(b)),
        )
      : item,
  );
}

/** Each reader owns its deadline and cancellation, including for shared GETs. */
export function createBrowserTransport(dispatch: Dispatch) {
  const pending = new Map<string, PendingRequest>();
  return async (url: string, options: FetchOptions = {}): Promise<unknown> => {
    // Reject backslashes, C0 controls and DEL before URL normalization.
    if (
      !url.startsWith("/") ||
      url.startsWith("//") ||
      url.includes("\\") ||
      Array.from(url).some((character) => {
        const code = character.charCodeAt(0);
        return code < 0x20 || code === 0x7f;
      })
    ) {
      throw new Error("API requests must use the current origin");
    }
    const { timeout, ...fetchOptions } = options;
    if (timeout !== undefined && (!Number.isFinite(timeout) || timeout < 0)) {
      throw new RangeError(
        "Request timeout must be a non-negative finite number",
      );
    }
    const context = sessionContext();
    const deadline = new AbortController();
    const signal = AbortSignal.any([
      context.signal,
      deadline.signal,
      ...(options.signal ? [options.signal] : []),
    ]);
    signal.throwIfAborted();
    const identity = { ...options };
    delete identity.signal;
    delete identity.headers;
    const key =
      (options.method ?? "GET").toUpperCase() === "GET" &&
      !Object.hasOwn(options, "headers") &&
      options.responseType !== "stream" &&
      !Object.values(options).some((value) => typeof value === "function")
        ? requestFingerprint(
            stableKey({
              url,
              generation: context.generation,
              options: identity,
            }),
          )
        : undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    if (timeout) {
      timer = setTimeout(() => {
        deadline.abort(
          new DOMException("The request timed out", "TimeoutError"),
        );
      }, timeout);
    }
    let request = key ? pending.get(key) : undefined;
    if (!request) {
      const controller = new AbortController();
      // ofetch disables its timeout when given a signal. The reader deadlines
      // above cover the complete operation, including response parsing/retries.
      // A shared dispatch ends only with its session or its last active reader.
      let response: Promise<BrowserResponse>;
      try {
        response = dispatch(url, {
          ...fetchOptions,
          credentials: "same-origin",
          baseURL: undefined,
          signal: AbortSignal.any([context.signal, controller.signal]),
        });
      } catch (error) {
        response = Promise.reject(error);
      }
      const created: PendingRequest = {
        controller,
        readers: 0,
        settled: false,
        response: response.then((result) =>
          result.status === 204 ||
          result._data === undefined ||
          result._data === ""
            ? null
            : result._data,
        ),
      };
      request = created;
      if (key) pending.set(key, created);
      const cleanup = () => {
        created.settled = true;
        if (key && pending.get(key) === created) pending.delete(key);
      };
      void created.response.then(cleanup, cleanup);
    }
    const current = request;
    current.readers++;
    return new Promise((resolve, reject) => {
      let finished = false;
      const cleanup = () => {
        finished = true;
        signal.removeEventListener("abort", abort);
        clearTimeout(timer);
        current.readers--;
        if (!current.readers && !current.settled) {
          if (key && pending.get(key) === current) pending.delete(key);
          current.controller.abort();
        }
      };
      const abort = () => {
        if (finished) return;
        cleanup();
        reject(
          signal.reason ??
            new DOMException("Session context changed", "AbortError"),
        );
      };
      signal.addEventListener("abort", abort, { once: true });
      if (signal.aborted) abort();
      void current.response.then(
        (value) => {
          if (finished) return;
          if (
            signal.aborted ||
            context.generation !== sessionContext().generation
          ) {
            abort();
            return;
          }
          cleanup();
          try {
            resolve(key ? structuredClone(value) : value);
          } catch (error) {
            reject(error);
          }
        },
        (error: unknown) => {
          if (finished) return;
          cleanup();
          reject(signal.aborted ? signal.reason : error);
        },
      );
    });
  };
}
