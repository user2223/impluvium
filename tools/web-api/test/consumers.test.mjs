import assert from 'node:assert/strict';
import test from 'node:test';
import { checkConsumer, checkConsumers } from '../check-consumers.mjs';

test('every UI consumer stays independent of backend types and obsolete fetch access', async () => {
  assert.ok(await checkConsumers() > 100);
});

test('the import boundary rejects types, runtime enums, dynamic imports and relative escapes', () => {
  for (const source of [
    'import type { User } from "~/prisma/client";',
    'import { Platform } from "~/prisma/client/enums";',
    'import type { Result } from "~/server/api/example";',
    'import type { SerializeObject } from "nitropack";',
    'const types = import("nitropack/types");',
    'import type { Result } from "../server/api/example";',
  ]) assert.throws(() => checkConsumer(source, '/sample/server/components/Example.vue'), /Backend import/);
  assert.throws(() => checkConsumer('$dropFetch("/api/v1");', 'sample.ts'), /Obsolete fetch/);
  assert.throws(() => checkConsumer('$apiFetch<Result>("GET /api/v1");', 'sample.ts'), /Free fetch return type/);
  assert.doesNotThrow(() => checkConsumer('import type { ApiResponse } from "~/shared/api/types";', 'sample.ts'));
});

test('browser consumers reject server context, forwarded headers and hydration paths', () => {
  for (const source of [
    'if (import.meta.server) forward();', 'if (import.meta.client) start();',
    'if (process.server) start();', 'if (process.client) start();',
    'useRequestHeader("cookie");', 'useRequestHeaders(["authorization"]);',
    'useRequestEvent();', 'useRequestFetch();', 'useRequestURL();',
    'useResponseHeader("set-cookie");', 'onServerPrefetch(fetchData);',
    'getRequestHeader(event, "cookie");', 'getRequestHeaders(event);',
    'useSSRContext();', 'onPrehydrate(callback);',
    'useHydration("sample", read, write);', 'defineLazyHydrationComponent("idle", load);',
    'app.ssrContext', 'app.isHydrating', 'app.payload.serverRendered',
    '<LazyHeader hydrate-on-idle />', '<Footer hydrate-on-interaction />',
    '<MenuItem hydrate-on-visible />', 'useState("api-fetch-transfer", () => []);',
    '<ClientOnly fallback-tag="span" />', '<client-only />',
    '<NuxtClientFallback />', '<nuxt-client-fallback />',
    'useState("api-fetch-credentials", () => ({}));',
  ]) assert.throws(() => checkConsumer(source, 'sample.vue'), /Server rendering or hydration/);
  for (const source of [
    '$apiFetch("GET /api/v1", { headers: { Authorization: token } });',
    'const cookie = document.cookie;', 'useState("notifications", () => []);',
    '<LazyUserHeader />',
  ]) assert.doesNotThrow(() => checkConsumer(source, 'sample.vue'));
  for (const source of [
    'interface TransferEntry {}', 'findTransfer(entries, key);',
    'transferBodyIdentity(body);', 'isTransferResponse(response);',
    'mergeRequestHeaders(forwarded, explicit);',
  ]) assert.throws(() => checkConsumer(source, 'sample.ts'), /Obsolete response transfer helper/);
});
