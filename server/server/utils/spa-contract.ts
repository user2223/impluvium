import { createHash } from "node:crypto";
import { readFile, readdir } from "node:fs/promises";
import { join } from "node:path";
import { load } from "cheerio";

export const hashBytes = (bytes: string | Buffer) =>
  createHash("sha256").update(bytes).digest("base64");
export interface SpaManifest {
  version: 1;
  files: Record<string, { size: number; sha256: string }>;
  scripts: Record<string, string[]>;
}

/** HTML's classic JavaScript MIME types, plus module and importmap scripts. */
export function inlineScriptNeedsHash(type?: string, language?: string) {
  const trim = (value: string) =>
    value.replace(/^[\t\n\f\r ]+|[\t\n\f\r ]+$/g, "");
  const value =
    type === undefined && language
      ? trim(`text/${language}`)
      : trim(type ?? "");
  if (value === "" || /^(?:module|importmap)$/i.test(value)) return true;
  return /^(?:application\/(?:x-)?(?:java|ecma)script|text\/(?:x-)?(?:java|ecma)script|text\/javascript1\.[0-5]|text\/(?:jscript|livescript))$/i.test(
    value,
  );
}

/** The browser hashes script text after HTML newline normalization. */
export function inlineHashes(html: string): string[] {
  const document = load(html.replace(/\r\n?/g, "\n"));
  if (document("style, [style]").length) {
    throw new Error("Static HTML contains inline styles");
  }
  const hashes = new Set<string>();
  document("script").each((_, element) => {
    const script = document(element);
    if (
      script.attr("src") === undefined &&
      inlineScriptNeedsHash(script.attr("type"), script.attr("language"))
    ) {
      hashes.add(`'sha256-${hashBytes(script.text())}'`);
    }
  });
  return [...hashes].sort();
}

export async function createSpaManifest(root: string): Promise<SpaManifest> {
  const manifest: SpaManifest = { version: 1, files: {}, scripts: {} };
  async function visit(directory: string, prefix = "") {
    const entries = (await readdir(directory, { withFileTypes: true })).sort(
      (a, b) => a.name.localeCompare(b.name),
    );
    for (const entry of entries) {
      if (entry.isSymbolicLink()) {
        throw new Error("SPA output must not contain symbolic links");
      }
      const name = prefix + entry.name;
      if (name === "csp-hashes.json") continue;
      if (entry.isDirectory())
        await visit(join(directory, entry.name), name + "/");
      else if (entry.isFile()) {
        const bytes = await readFile(join(root, name));
        manifest.files[name] = { size: bytes.length, sha256: hashBytes(bytes) };
        if (name.endsWith(".html")) {
          manifest.scripts[name] = inlineHashes(bytes.toString("utf8"));
        }
      } else throw new Error("Unsupported SPA output entry");
    }
  }
  await visit(root);
  if (!manifest.files["index.html"] || !manifest.files["loading.css"]) {
    throw new Error("SPA output is missing index.html or loading.css");
  }
  return manifest;
}

/** Refuse mismatched HTML, scripts, assets, missing files and extra files. */
export async function validateSpaManifest(root: string): Promise<SpaManifest> {
  const actual: unknown = JSON.parse(
    await readFile(join(root, "csp-hashes.json"), "utf8"),
  );
  const expected = await createSpaManifest(root);
  const canonical = (value: unknown): string =>
    JSON.stringify(value, (_, item) =>
      item && typeof item === "object" && !Array.isArray(item)
        ? Object.fromEntries(
            Object.entries(item).sort(([a], [b]) => a.localeCompare(b)),
          )
        : item,
    );
  if (canonical(actual) !== canonical(expected)) {
    throw new Error("SPA hash manifest does not match the output");
  }
  return expected;
}

/** Decode exactly once before both classification and lookup. Reject ambiguity. */
export function canonicalSpaPath(raw: string): string {
  const path = raw.split("?")[0];
  if (!path.startsWith("/") || /%(?:2f|5c)/i.test(path)) {
    throw new Error("Invalid request path");
  }
  const decoded = decodeURIComponent(path);
  // Reject C0 controls and DEL in both literal and percent-encoded paths.
  if (
    /[\\%;]/.test(decoded) ||
    Array.from(decoded).some((character) => {
      const code = character.charCodeAt(0);
      return code < 0x20 || code === 0x7f;
    }) ||
    decoded.split("/").some((segment) => segment === "..")
  ) {
    throw new Error("Invalid request path");
  }
  return (
    "/" +
    decoded
      .split("/")
      .filter((segment) => segment && segment !== ".")
      .join("/")
  );
}

export function isServerPath(path: string) {
  return (
    path === "/api" ||
    path.startsWith("/api/") ||
    path === "/_nitro" ||
    path.startsWith("/_nitro/") ||
    path === "/auth/oidc" ||
    path === "/auth/signout" ||
    path === "/auth/callback/oidc"
  );
}

/** A namespace miss must stay an API error rather than reach Nuxt's renderer. */
export function matchesApiRoute(
  path: string,
  method: string,
  routes: readonly { path: string; method: string }[],
) {
  const segments = path.replace(/\/$/, "").split("/");
  return routes.some((route) => {
    if (
      route.method !== method &&
      !(method === "HEAD" && route.method === "GET")
    ) {
      return false;
    }
    const template = route.path.split("/");
    return (
      template.length === segments.length &&
      template.every(
        (segment, index) =>
          segment.startsWith(":") || segment === segments[index],
      )
    );
  });
}

export function spaFile(
  path: string,
  files: SpaManifest["files"],
): string | undefined {
  const file = path.slice(1);
  if (Object.hasOwn(files, file) && !/\.(?:gz|br)$/.test(file)) return file;
  if (
    ["/_nuxt", "/_fonts", "/fonts", "/wallpapers"].some(
      (prefix) => path === prefix || path.startsWith(prefix + "/"),
    ) ||
    /\.(?:html?|m?js|css|json|map|png|jpe?g|gif|webp|avif|svg|ico|ttf|woff2?|txt|wasm|gz|br)$/i.test(
      path,
    )
  ) {
    return undefined;
  }
  return "index.html";
}

export function spaSecurityHeaders(hashes: string[]) {
  return {
    "Content-Security-Policy": [
      "default-src 'self'",
      `script-src 'self' ${hashes.join(" ")}`.trim(),
      "script-src-attr 'none'",
      "style-src 'self'",
      "style-src-attr 'none'",
      "connect-src 'self'",
      "font-src 'self'",
      "object-src 'none'",
      "base-uri 'none'",
      "form-action 'self'",
      "frame-ancestors 'self'",
      "frame-src 'self'",
      "img-src 'self' data: https://www.giantbomb.com https://images.pcgamingwiki.com https://images.igdb.com https://*.steamstatic.com",
    ].join("; "),
    "X-Content-Type-Options": "nosniff",
    "X-Frame-Options": "SAMEORIGIN",
    "Referrer-Policy": "no-referrer",
    "Permissions-Policy":
      "camera=(), display-capture=(), fullscreen=(), geolocation=(), microphone=()",
    "Cross-Origin-Opener-Policy": "same-origin",
    "Cross-Origin-Embedder-Policy": "credentialless",
    "Cross-Origin-Resource-Policy": "same-origin",
    "Origin-Agent-Cluster": "?1",
    "X-DNS-Prefetch-Control": "off",
    "X-Download-Options": "noopen",
    "X-Permitted-Cross-Domain-Policies": "none",
    "X-XSS-Protection": "0",
  };
}

export function spaCacheControl(file: string) {
  const name = file.slice(file.lastIndexOf("/") + 1);
  const hashed =
    file.startsWith("_nuxt/") &&
    (/^[\w-]{8,}\.(?:m?js|css)(?:\.map)?$/.test(name) ||
      /[.-][\w-]{8,}\./.test(name));
  return !file.endsWith(".html") && hashed
    ? "public, max-age=31536000, immutable"
    : "no-cache";
}
