import type {
  ExtractedRouteMethod,
  NitroFetchOptions,
  NitroFetchRequest,
  TypedInternalResponse,
} from "nitropack/types";
import type {
  $Fetch as TransportFetch,
  FetchError,
  FetchOptions,
  ResponseType,
} from "ofetch";
import type { ApiFetch, ApiFetchKey, ApiOptions } from "~/shared/api/client";
import { apiRoutes } from "~/shared/api/generated/routes";
import {
  assertApiFetchOptions,
  findTransfer,
  isBinaryRequestBody,
  isBinaryValue,
  isTransferResponse,
  mergeRequestHeaders,
  requestFingerprint,
  resolveApiPath,
  stableRequestKey,
  transferBodyIdentity,
  type TransferEntry,
} from "~/shared/api/request";

interface DropFetch<
  DefaultT = unknown,
  DefaultR extends NitroFetchRequest = NitroFetchRequest,
> {
  <
    T = DefaultT,
    R extends NitroFetchRequest = DefaultR,
    O extends NitroFetchOptions<R> = NitroFetchOptions<R>,
  >(
    request: R,
    opts?: O & { failTitle?: string; params?: { [key: string]: string } },
  ): Promise<
    // sometimes there is an error, other times there isn't
    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    // @ts-ignore
    TypedInternalResponse<
      R,
      T,
      NitroFetchOptions<R> extends O ? "get" : ExtractedRouteMethod<R, O>
    >
  >;
}

type TransportOptions = FetchOptions & { failTitle?: string };

// Each SSR request has its own Nuxt app. Binary snapshots stay available for
// further copies for that app's lifetime, never in the payload or another app.
interface PendingServerRead {
  response: Promise<unknown>;
  readers: number;
  stream?: ReadableStream<unknown>;
  retain?: boolean;
}
const pendingReads = new WeakMap<object, Map<string, PendingServerRead>>();
const hydrationCleanups = new WeakSet<object>();

function copyTransferValue(value: unknown): unknown {
  if (!isTransferResponse(value)) {
    throw new Error("Only JSON, text and empty responses can be transferred");
  }
  return value === undefined ? undefined : JSON.parse(JSON.stringify(value));
}

async function shareServerRead(
  app: object,
  key: string,
  fetchData: () => Promise<unknown>,
): Promise<unknown> {
  let reads = pendingReads.get(app);
  if (!reads) {
    reads = new Map();
    pendingReads.set(app, reads);
  }
  let pending = reads.get(key);
  const ownsRequest = !pending;
  if (!pending) {
    pending = { response: fetchData(), readers: 0 };
    reads.set(key, pending);
  }
  pending.readers++;
  try {
    const value = await pending.response;
    if (value === undefined || value === null) return value;
    if (!isBinaryValue(value)) {
      if (isTransferResponse(value)) {
        return ownsRequest ? value : copyTransferValue(value);
      }
      // Opaque legacy parser/hook results retain their type. Other consumers
      // fetch their own result rather than inventing a JSON copy of it.
      return ownsRequest ? value : await fetchData();
    }
    pending.retain = true;
    if (typeof File !== "undefined" && value instanceof File) {
      return new File([value], value.name, {
        type: value.type,
        lastModified: value.lastModified,
      });
    }
    if (value instanceof Blob) {
      return value.slice(0, value.size, value.type);
    }
    if (
      value instanceof ArrayBuffer ||
      (typeof SharedArrayBuffer !== "undefined" &&
        value instanceof SharedArrayBuffer)
    ) {
      return value.slice(0);
    }
    if (value instanceof DataView) {
      const buffer = value.buffer.slice(
        value.byteOffset,
        value.byteOffset + value.byteLength,
      );
      return new DataView(buffer);
    }
    if (ArrayBuffer.isView(value)) {
      // Typed-array constructors copy elements; Buffer.from copies Node buffers
      // without Buffer.slice's shared memory or a deprecated Buffer constructor.
      const constructor = value.constructor;
      if (constructor.name === "Buffer") {
        const buffer = value.buffer.slice(
          value.byteOffset,
          value.byteOffset + value.byteLength,
        );
        return Reflect.apply(Reflect.get(constructor, "from"), constructor, [
          buffer,
        ]);
      }
      return Reflect.construct(constructor, [value]);
    }
    if (value instanceof ReadableStream) {
      const stream = pending.stream ?? value;
      // Keep a branch for later readers in this server request, even after all
      // current readers finish. The app-scoped WeakMap owns the retained branch.
      const [branch, remaining] = stream.tee();
      pending.stream = remaining;
      return branch;
    }
    throw new Error("Unsupported binary response value");
  } finally {
    pending.readers--;
    if (!pending.readers && !pending.retain) {
      reads.delete(key);
      if (!reads.size) pendingReads.delete(app);
    }
  }
}

// Keep Nitro inference at the legacy interface, outside the transport body.
export const $dropFetch = legacyDropFetch as unknown as DropFetch;

async function legacyDropFetch(
  rawRequest: NitroFetchRequest,
  opts?: TransportOptions & { params?: Record<string, string> },
): Promise<unknown> {
  const parameters = new Map<string, string>(
    Object.entries(opts?.params ?? {}),
  );
  const requestParts = rawRequest.toString().split("/");
  requestParts.forEach((part, index) => {
    if (!part.startsWith(":")) {
      return;
    }
    const partName = part.slice(1);
    const replacement = parameters.get(partName);
    if (!replacement) {
      return;
    }
    requestParts[index] = replacement;

    parameters.delete(partName);
  });
  const request = requestParts.join("/");

  return dropTransport(request, {
    ...opts,
    params: Object.fromEntries(parameters),
  });
}

export const $apiFetch: ApiFetch = (key, ...args) => {
  const route = apiRoutes[key];
  if (!route) throw new Error(`Unknown API route: ${key}`);
  const options = (args[0] ?? {}) as ApiOptions<ApiFetchKey>;
  assertApiFetchOptions(options);
  const { path, ...fetchOptions } = options;
  const request = resolveApiPath(route.path, path);
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

/**
 * Shared SSR/hydration policy for the complete ofetch body/response surface:
 *
 * Body kind                         Rule
 * undefined/null                    Empty identity; normal response transfer.
 * String/number/boolean/array/record JSON/text identity; normal transfer.
 * toJSON object                     ofetch JSON identity; normal transfer.
 * JSON + form-urlencoded header     ofetch's ordered form-field identity.
 * URLSearchParams                   Ordered field identity; normal transfer.
 * FormData                          Text/file metadata identity, never bytes.
 * Blob/File, buffers/views, streams  Reject render-time mutations before send.
 * Node streams / iterable bytes     Same rejection, without reading chunks.
 * Other/unsupported bodies          Reject render-time mutations; otherwise
 *                                   pass unchanged to ofetch for validation.
 * Legacy callbacks/custom dispatch  Not keyable across SSR and browser: no
 *                                   read sharing/transfer; reject render-time
 *                                   mutations, pass client events unchanged.
 *
 * Actual response kind              Rule (regardless of responseType option)
 * JSON/text/empty                    Share SSR reads; FIFO mutation transfer.
 * Blob/File/buffer/view/web stream   Never payload/JSON: clone or tee app-scoped
 *                                   SSR reads; browser fetches normally.
 * Opaque legacy parser/hook value    Preserve unchanged, no transfer or sharing.
 *
 * Declared binary mutation responses are rejected before render-time dispatch.
 * A late binary/opaque mutation result leaves a metadata-only blocking slot:
 * hydration rejects it before retrying an already executed side effect.
 * After hydration, client events pass all bodies/results through unchanged.
 */
async function dropTransport<T>(
  request: string,
  options: TransportOptions = {},
): Promise<T> {
  // Capture the Nuxt context before awaiting, also outside component setup.
  const app = tryUseNuxtApp();
  const inSetup = !!getCurrentInstance()?.proxy;
  const notifyOnFailure = import.meta.client && !inSetup;
  const forwarded = new Headers();
  if (import.meta.server && app) {
    const incoming = useRequestHeaders(["cookie", "authorization"]);
    for (const [name, value] of Object.entries(incoming)) {
      if (typeof value === "string") forwarded.set(name, value);
    }
  }
  const transfer = app
    ? useState<TransferEntry[]>("api-fetch-transfer", () => [])
    : undefined;
  // The browser needs the same normalization as SSR, including when callers
  // explicitly repeat an incoming credential. Transfer fingerprints, not values.
  const credentialFingerprints = app
    ? useState<Record<string, string>>("api-fetch-credentials", () =>
        Object.fromEntries(
          [...forwarded].map(([name, value]) => [
            name,
            requestFingerprint(value),
          ]),
        ),
      )
    : undefined;
  const method = (options.method ?? "GET").toUpperCase();
  const sharedRead = method === "GET" || method === "HEAD";
  const declaredBinaryResponse =
    options.responseType === "blob" ||
    options.responseType === "arrayBuffer" ||
    options.responseType === "stream";
  const renderMutation =
    !sharedRead && (import.meta.server || inSetup || app?.isHydrating);
  // Callback closures and custom dispatchers cannot be represented by a stable
  // cross-runtime identity. In particular, a custom parser must not receive a
  // cached value produced by a different parser (or by default parsing).
  const customBehavior =
    options.parseResponse !== undefined ||
    options.onRequest !== undefined ||
    options.onRequestError !== undefined ||
    options.onResponse !== undefined ||
    options.onResponseError !== undefined ||
    typeof options.retryDelay === "function" ||
    options.dispatcher !== undefined ||
    options.agent !== undefined;
  if (renderMutation && isBinaryRequestBody(options.body)) {
    throw new Error(
      "Binary mutation bodies cannot be transferred through hydration; run this mutation from a client event after hydration and outside component setup.",
    );
  }
  if (renderMutation && declaredBinaryResponse) {
    // Binary responses cannot enter the page payload. Repeating a mutation to
    // recover its missing response would apply its side effects twice, so reject
    // before dispatch in SSR/setup/hydration. Client event handlers may use it.
    throw new Error(
      "Binary mutation responses cannot be transferred through hydration; run this mutation from a client event after hydration and outside component setup.",
    );
  }
  if (renderMutation && customBehavior) {
    throw new Error(
      "Custom fetch callbacks or dispatchers cannot be replayed during hydration; run this mutation from a client event after hydration and outside component setup.",
    );
  }
  const bodyIdentity = transferBodyIdentity(
    options.body,
    new Headers(options.headers).get("content-type"),
  );
  if (renderMutation && bodyIdentity === undefined) {
    throw new Error(
      "This mutation body cannot be identified for hydration; run it from a client event after hydration and outside component setup.",
    );
  }
  const { failTitle, ...fetchOptions } = options;
  let key: string | undefined;
  let mutationEntry: TransferEntry | undefined;
  if (transfer && import.meta.client && app) {
    if (!app.isHydrating) {
      transfer.value = [];
      if (credentialFingerprints) credentialFingerprints.value = {};
    } else if (!hydrationCleanups.has(app)) {
      hydrationCleanups.add(app);
      app.hook("app:suspense:resolve", () => {
        transfer.value = [];
        if (credentialFingerprints) credentialFingerprints.value = {};
      });
    }
  }
  if (
    transfer &&
    !customBehavior &&
    bodyIdentity !== undefined &&
    (import.meta.server || app?.isHydrating)
  ) {
    // Only explicit header overrides enter the key; forwarded cookies stay local.
    // Hashing keeps credentials out of the serialized state keys.
    const explicit = new Headers(options.headers);
    for (const name of ["cookie", "authorization"]) {
      const value = explicit.get(name);
      if (
        value !== null &&
        requestFingerprint(value) === credentialFingerprints?.value[name]
      ) {
        explicit.delete(name);
      }
    }
    const identity = stableRequestKey({
      request,
      method,
      baseURL: options.baseURL,
      responseType: options.responseType ?? "auto",
      query: { ...options.params, ...options.query },
      body: bodyIdentity,
      headers: [...explicit.entries()].sort(),
    });
    key = requestFingerprint(identity);
    // Reads are shared on the server and remain reusable throughout hydration.
    // Mutations always run on the server; the browser consumes their slots FIFO.
    if (sharedRead || import.meta.client) {
      const entry = findTransfer(transfer.value, key);
      if (entry) {
        if (entry.blocked) {
          // Retain this blocker for the rest of hydration, including retries.
          throw new Error(
            `Mutation response cannot be replayed during hydration (${entry.blocked}); run it from a client event after hydration.`,
          );
        }
        if (!sharedRead) {
          transfer.value.splice(transfer.value.indexOf(entry), 1);
        }
        if (entry.ready !== false) {
          return copyTransferValue(entry.value) as T;
        }
      }
    }
    if (import.meta.server && !sharedRead) {
      // Reserve before awaiting so completion order cannot reorder mutations.
      // A failed slot stays unavailable: its browser call must perform a fetch
      // rather than consume the next mutation's successful response.
      mutationEntry = { key, value: undefined, ready: false };
      transfer.value.push(mutationEntry);
    }
  }
  try {
    const fetchData = async (): Promise<T> => {
      // Retain Nitro's local SSR dispatch with ofetch's runtime signature.
      const fetch = $fetch as unknown as TransportFetch;
      const data = await fetch<T, ResponseType>(request, {
        ...fetchOptions,
        headers: mergeRequestHeaders(forwarded, options.headers),
      });
      if (import.meta.server && transfer && key) {
        if (isTransferResponse(data)) {
          const value = copyTransferValue(data);
          if (mutationEntry) {
            mutationEntry.value = value;
            mutationEntry.ready = true;
          } else {
            transfer.value.push({ key, value, ready: true });
          }
        } else if (mutationEntry) {
          // Never put the result itself in the payload, and never replay its
          // already completed mutation to obtain an untransferable result.
          mutationEntry.blocked = isBinaryValue(data) ? "binary" : "opaque";
        }
      }
      return data as T;
    };
    const data =
      import.meta.server && app && key && sharedRead
        ? await shareServerRead(app, key, fetchData)
        : await fetchData();
    return data as T;
  } catch (error) {
    if (notifyOnFailure && failTitle) {
      console.warn(error);
      createModal(
        ModalType.Notification,
        {
          title: failTitle,
          description: (error as FetchError)?.data?.message ?? String(error),
        },
        (_, close) => close(),
      );
    }
    throw error;
  }
}

export function isClientRequest() {
  const existingState = useState("clientMode", () => false);
  if (import.meta.server) {
    const headers = useRequestHeaders(["User-Agent"]);
    const calculatedClientRequest =
      headers["user-agent"]?.startsWith("impluvium-desktop/") ?? false;
    existingState.value = calculatedClientRequest;
  }

  return existingState.value;
}
