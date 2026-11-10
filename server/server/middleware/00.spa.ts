import { readFile } from "node:fs/promises";
import { join, extname } from "node:path";
import {
  canonicalSpaPath,
  isServerPath,
  matchesApiRoute,
  spaFile,
  spaCacheControl,
  spaSecurityHeaders,
} from "../utils/spa-contract";
import { spaOutputManifest, spaRoot } from "../utils/spa-output";
import { apiRoutes } from "../../shared/api/generated/routes";

const routes = Object.values(apiRoutes);

const contentTypes: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".webp": "image/webp",
  ".avif": "image/avif",
  ".ico": "image/x-icon",
  ".ttf": "font/ttf",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
  ".txt": "text/plain; charset=utf-8",
  ".map": "application/json",
  ".wasm": "application/wasm",
};

export default defineEventHandler(async (event) => {
  if (import.meta.dev) return;
  const failure = (status: number, message: string) => {
    setHeaders(event, spaSecurityHeaders([]));
    setResponseStatus(event, status);
    setHeader(event, "Content-Type", "text/plain; charset=utf-8");
    setHeader(event, "Cache-Control", "no-cache");
    return send(event, message);
  };
  // These routes retain their handlers, status codes, queries and upgrade paths.
  // In particular, encoded API parameters belong to Nitro, not the file server.
  const rawPath = (event.node.req.url ?? "/").split("?")[0];
  if (isServerPath(rawPath) || getHeader(event, "upgrade")) {
    setHeaders(event, spaSecurityHeaders([]));
    if (
      (rawPath === "/api" || rawPath.startsWith("/api/")) &&
      event.method !== "OPTIONS" &&
      !matchesApiRoute(rawPath, event.method, routes)
    ) {
      setResponseStatus(event, 404);
      setHeader(event, "Content-Type", "application/json; charset=utf-8");
      return send(
        event,
        JSON.stringify({ statusCode: 404, statusMessage: "Not Found" }),
      );
    }
    return;
  }
  let path: string;
  try {
    path = canonicalSpaPath(rawPath);
  } catch {
    return failure(400, "Invalid request path");
  }
  if (isServerPath(path)) return failure(400, "Invalid server route path");
  const manifest = spaOutputManifest();
  if (event.method !== "GET" && event.method !== "HEAD") {
    setHeader(event, "Allow", "GET, HEAD");
    return failure(405, "Method not allowed");
  }
  const selected = spaFile(path, manifest.files);
  if (!selected) return failure(404, "Asset not found");
  let file = selected;
  setHeaders(event, spaSecurityHeaders(manifest.scripts[file] ?? []));
  setHeader(event, "Cache-Control", spaCacheControl(file));
  setHeader(
    event,
    "Content-Type",
    contentTypes[extname(file)] ?? "application/octet-stream",
  );
  setHeader(event, "Vary", "Accept-Encoding");
  const accepted = (getHeader(event, "accept-encoding") ?? "")
    .split(",")
    .map((entry) => {
      const [name, ...parameters] = entry.trim().split(";");
      const quality = parameters.find((part) => part.trim().startsWith("q="));
      return { name, quality: quality ? Number(quality.trim().slice(2)) : 1 };
    });
  const encoding = ["br", "gzip"].find(
    (name) =>
      accepted.some((entry) => entry.name === name && entry.quality > 0) &&
      Object.hasOwn(manifest.files, file + (name === "br" ? ".br" : ".gz")),
  );
  if (encoding) {
    setHeader(event, "Content-Encoding", encoding);
    file += encoding === "br" ? ".br" : ".gz";
  }
  setHeader(event, "Content-Length", manifest.files[file].size);
  return event.method === "HEAD"
    ? send(event, "")
    : send(event, await readFile(join(spaRoot, file)));
});
