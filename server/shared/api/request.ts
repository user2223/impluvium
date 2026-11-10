/** Resolve and encode the parameters of an explicit route template. */
export function resolveApiPath(
  template: string,
  parameters: Readonly<Record<string, string | number>> = {},
): string {
  const used = new Set<string>();
  const path = template.replace(/:([A-Za-z0-9_]+)/g, (_, name: string) => {
    if (!Object.hasOwn(parameters, name)) {
      throw new Error(`Missing path parameter: ${name}`);
    }
    const value = parameters[name];
    if (
      value === "" ||
      value === "." ||
      value === ".." ||
      (typeof value !== "string" && typeof value !== "number")
    ) {
      throw new Error(`Invalid path parameter: ${name}`);
    }
    used.add(name);
    return encodeURIComponent(String(value));
  });
  for (const name of Object.keys(parameters)) {
    if (!used.has(name)) throw new Error(`Unexpected path parameter: ${name}`);
  }
  return path;
}

/** Reject options that could mutate a typed request after its identity is set. */
export function assertApiFetchOptions(options: object): void {
  for (const name of [
    "onRequest",
    "onRequestError",
    "onResponse",
    "onResponseError",
    "method",
    "params",
    "responseType",
    "baseURL",
    "parseResponse",
    "ignoreResponseError",
    "dispatcher",
    "agent",
  ]) {
    if (Reflect.get(options, name) !== undefined) {
      throw new Error(`Unsupported typed request option: ${name}`);
    }
  }
  const retryDelay = Reflect.get(options, "retryDelay");
  if (retryDelay !== undefined && typeof retryDelay !== "number") {
    throw new Error("Typed request retryDelay must be a number");
  }
}

/** Explicit headers override forwarded headers, independent of casing. */
export function mergeRequestHeaders(
  forwarded: HeadersInit | undefined,
  explicit: HeadersInit | undefined,
): Headers {
  const headers = new Headers(forwarded);
  new Headers(explicit).forEach((value, name) => headers.set(name, value));
  return headers;
}

export function stableRequestKey(value: unknown): string {
  return (
    JSON.stringify(value, (_, item: unknown) => {
      if (item && typeof item === "object" && !Array.isArray(item)) {
        return Object.fromEntries(
          Object.entries(item).sort(([a], [b]) => a.localeCompare(b)),
        );
      }
      return item;
    }) ?? "undefined"
  );
}

/** Runtime values that must never be JSON-serialized into a transfer payload. */
export function isBinaryValue(value: unknown): boolean {
  try {
    return (
      value instanceof Blob ||
      value instanceof ArrayBuffer ||
      (typeof SharedArrayBuffer !== "undefined" &&
        value instanceof SharedArrayBuffer) ||
      ArrayBuffer.isView(value) ||
      value instanceof ReadableStream
    );
  } catch {
    return false;
  }
}

/** FormData files are metadata-keyed; a standalone binary body is not replayable. */
export function isBinaryRequestBody(body: unknown): boolean {
  if (isBinaryValue(body)) return true;
  if (
    !body ||
    typeof body !== "object" ||
    Array.isArray(body) ||
    body instanceof FormData ||
    body instanceof URLSearchParams ||
    Object.getPrototypeOf(body) === Object.prototype ||
    Object.getPrototypeOf(body) === null
  ) {
    return false;
  }
  // ofetch additionally recognizes Node streams. Native fetch can also accept
  // iterable byte sources; inspect their methods without reading a single chunk.
  if (
    typeof Reflect.get(body, "pipe") === "function" ||
    typeof Reflect.get(body, "pipeTo") === "function"
  ) {
    return true;
  }
  if (typeof Reflect.get(body, "toJSON") === "function") return false;
  return (
    typeof Reflect.get(body, Symbol.iterator) === "function" ||
    typeof Reflect.get(body, Symbol.asyncIterator) === "function"
  );
}

/** Only actual JSON/text/empty values may enter the hydration payload. */
export function isTransferResponse(value: unknown): boolean {
  const pending = [{ value, root: true }];
  const seen = new Set<object>();
  try {
    while (pending.length) {
      const current = pending.pop();
      if (!current) return false;
      const item = current.value;
      if (item === undefined && current.root) continue;
      if (
        item === null ||
        typeof item === "string" ||
        typeof item === "boolean"
      ) {
        continue;
      }
      if (typeof item === "number" && Number.isFinite(item)) continue;
      if (!item || typeof item !== "object" || isBinaryValue(item)) {
        return false;
      }
      if (seen.has(item)) return false;
      seen.add(item);
      const array = Array.isArray(item);
      if (
        Object.getPrototypeOf(item) !==
        (array ? Array.prototype : Object.prototype)
      ) {
        return false;
      }
      const keys = Reflect.ownKeys(item);
      if (array && keys.length !== item.length + 1) return false;
      for (const key of keys) {
        if (array && key === "length") continue;
        if (typeof key !== "string") return false;
        if (
          array &&
          (String(Number(key)) !== key ||
            !Number.isInteger(Number(key)) ||
            Number(key) < 0 ||
            Number(key) >= item.length)
        ) {
          return false;
        }
        const field = Object.getOwnPropertyDescriptor(item, key);
        if (!field?.enumerable || !("value" in field)) return false;
        pending.push({ value: field.value, root: false });
      }
    }
    return true;
  } catch {
    // Custom legacy parsers can return arbitrary objects, including proxies.
    // Preserve their values instead of calling custom serializers or coercing them.
    return false;
  }
}

/** Identify supported bodies without reading or serializing upload contents. */
export function transferBodyIdentity(
  body: unknown,
  contentType?: string | null,
): unknown {
  if (isBinaryRequestBody(body)) return undefined;
  if (body instanceof FormData) {
    return {
      format: "form-data",
      entries: [...body.entries()].map(([name, value]) =>
        typeof value === "string"
          ? { name, text: value }
          : {
              name,
              file: { name: value.name, size: value.size, type: value.type },
            },
      ),
    };
  }
  if (body instanceof URLSearchParams) {
    return { format: "url-search-params", entries: [...body.entries()] };
  }
  if (body === undefined || body === null) {
    return { format: "empty", value: body };
  }
  // ofetch sends strings verbatim but JSON-stringifies other JSON candidates.
  // Keeping these formats distinct prevents toJSON strings colliding with raw text.
  if (typeof body === "string") return { format: "text", value: body };
  if (
    typeof body === "number" ||
    typeof body === "boolean" ||
    Array.isArray(body) ||
    (typeof body === "object" &&
      (Object.getPrototypeOf(body) === Object.prototype ||
        Object.getPrototypeOf(body) === null ||
        typeof Reflect.get(body, "toJSON") === "function"))
  ) {
    if (contentType === "application/x-www-form-urlencoded" && body) {
      const form: URLSearchParams = Reflect.construct(URLSearchParams, [body]);
      return { format: "url-search-params", entries: [...form.entries()] };
    }
    return { format: "json", value: body };
  }
  return undefined;
}

/** SHA-256 also works on HTTP origins where Web Crypto is unavailable. */
export function requestFingerprint(value: string): string {
  const constants = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1,
    0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
    0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
    0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
    0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
  ];
  const bytes = new TextEncoder().encode(value);
  const buffer = new ArrayBuffer(Math.ceil((bytes.length + 9) / 64) * 64);
  const padded = new Uint8Array(buffer);
  padded.set(bytes);
  padded[bytes.length] = 0x80;
  const view = new DataView(buffer);
  view.setUint32(
    buffer.byteLength - 8,
    Math.floor((bytes.length * 8) / 2 ** 32),
  );
  view.setUint32(buffer.byteLength - 4, bytes.length * 8);
  const hash = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c,
    0x1f83d9ab, 0x5be0cd19,
  ];
  const rotate = (word: number, bits: number) =>
    (word >>> bits) | (word << (32 - bits));
  const words = new Uint32Array(64);
  for (let offset = 0; offset < buffer.byteLength; offset += 64) {
    for (let index = 0; index < 16; index++) {
      words[index] = view.getUint32(offset + index * 4);
    }
    for (let index = 16; index < 64; index++) {
      const x = words[index - 15];
      const y = words[index - 2];
      const s0 = rotate(x, 7) ^ rotate(x, 18) ^ (x >>> 3);
      const s1 = rotate(y, 17) ^ rotate(y, 19) ^ (y >>> 10);
      words[index] = words[index - 16] + s0 + words[index - 7] + s1;
    }
    let [a, b, c, d, e, f, g, h] = hash;
    for (let index = 0; index < 64; index++) {
      const s1 = rotate(e, 6) ^ rotate(e, 11) ^ rotate(e, 25);
      const choice = (e & f) ^ (~e & g);
      const t1 = (h + s1 + choice + constants[index] + words[index]) | 0;
      const s0 = rotate(a, 2) ^ rotate(a, 13) ^ rotate(a, 22);
      const majority = (a & b) ^ (a & c) ^ (b & c);
      [a, b, c, d, e, f, g, h] = [
        (t1 + s0 + majority) | 0,
        a,
        b,
        c,
        (d + t1) | 0,
        e,
        f,
        g,
      ];
    }
    for (const [index, word] of [a, b, c, d, e, f, g, h].entries()) {
      hash[index] = (hash[index] + word) >>> 0;
    }
  }
  return hash.map((word) => word.toString(16).padStart(8, "0")).join("");
}

export interface TransferEntry {
  key: string;
  value: unknown;
  // A reserved mutation slot has no response until its request succeeds.
  ready?: boolean;
  // A late-discovered non-transferable result must never cause a mutation retry.
  blocked?: "binary" | "opaque";
}

/** Falsy values and empty responses remain valid transfer entries. */
export function findTransfer(entries: readonly TransferEntry[], key: string) {
  return entries.find((entry) => entry.key === key);
}
