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
