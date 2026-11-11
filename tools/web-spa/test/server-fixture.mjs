import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { Writable } from 'node:stream';
import { compiler } from '../../web-api/compiler.mjs';
import { rootDirectory as root } from '../../web-api/generate.mjs';

export async function loadServerSource(file, modules, globals = {}) {
  const ts = compiler(root);
  const source = await readFile(join(root, file), 'utf8');
  const code = ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.CommonJS, esModuleInterop: true },
  }).outputText;
  const exports = {};
  new Function('require', 'exports', ...Object.keys(globals), code)((name) => {
    assert.ok(Object.hasOwn(modules, name), `Missing mock for ${name} in ${file}`);
    return modules[name];
  }, exports, ...Object.values(globals));
  return exports;
}

export async function databaseSessionFixture() {
  const require = createRequire(join(root, 'server/package.json'));
  const token = 'sample-session-token';
  const session = {
    authenticated: { userId: 'sample-user', level: 10, requiredLevel: 10 },
    expiresAt: new Date(Date.now() + 3600000).toISOString(),
    data: { preference: 'sample-value' },
  };
  const state = { reads: 0, searches: [], row: { token, userId: 'sample-user', expiresAt: new Date(session.expiresAt), data: session } };
  const values = new Map();
  const cacheFor = (namespace) => ({
    get: async (key) => values.has(`${namespace}:${key}`) ? JSON.parse(values.get(`${namespace}:${key}`)) : null,
    set: async (key, value) => { values.set(`${namespace}:${key}`, JSON.stringify(value)); },
    remove: async (key) => { values.delete(`${namespace}:${key}`); },
  });
  const cache = cacheFor('DBSession:v2');
  const prisma = { session: {
    findUnique: async ({ where }) => {
      assert.equal(where.token, token);
      state.reads++;
      return structuredClone(state.row);
    },
    findMany: async ({ where }) => {
      state.searches.push(where);
      return state.row ? [structuredClone(state.row)] : [];
    },
    upsert: async ({ where, create, update }) => {
      assert.equal(where.token, token);
      const data = state.row ? update : create;
      state.row = { token, userId: 'sample-user', expiresAt: new Date(data.expiresAt), data: JSON.parse(JSON.stringify(data.data)) };
      return structuredClone(state.row);
    },
    deleteMany: async ({ where }) => {
      assert.equal(where.token, token);
      const count = state.row ? 1 : 0;
      state.row = null;
      return { count };
    },
  } };
  const { default: createProvider } = await loadServerSource('server/server/internal/session/db.ts', {
    luxon: require('luxon'),
    '../db/database': prisma,
    '../cache': { createCache: cacheFor },
  });
  const { default: handler } = await loadServerSource('server/server/internal/session/index.ts', {
    'node:crypto': require('node:crypto'),
    'cookie-es': require('cookie-es'),
    luxon: require('luxon'),
    '../db/database': prisma,
    './db': createProvider,
  });
  return { provider: createProvider(), handler, state, cache, cacheFor, token, session };
}

export async function taskSocketFixture({ sessionHandler } = {}) {
  const state = {
    session: undefined,
    tokens: new Map(),
    task: { id: 'sample-task', name: 'Sample task', acls: ['system:task:read'], log: [], actions: [], progress: 0, success: false },
    taskQueries: 0,
  };
  const prisma = {
    aPIToken: { findUnique: async ({ where }) => {
      const token = state.tokens.get(where.token);
      if (where.mode?.in && !where.mode.in.includes(token?.mode)) return null;
      return token ?? null;
    } },
    user: { findUnique: async () => ({ id: 'sample-user', admin: state.admin ?? false }) },
    task: { findFirst: async ({ where }) => {
      state.taskQueries++;
      if (state.findTask) return state.findTask(where.id);
      return state.task?.id === where.id ? state.task : null;
    } },
  };
  const { default: aclManager } = await loadServerSource('server/server/internal/acls/index.ts', {
    '~/prisma/client/enums': { APITokenMode: { User: 'User', System: 'System', Client: 'Client' } },
    '../db/database': prisma,
    '../session': sessionHandler ?? { getSession: async () => state.session },
  });
  const modules = {
    '../acls': aclManager,
    './group': { taskGroups: {} },
    '../db/database': prisma,
    arktype: { type: () => (value) => value, ArkErrors: class {} },
    pino: () => {},
    '~/server/internal/logging': { logger: {} },
    'node:stream': { Writable },
  };
  for (const name of ['invitations', 'sessions', 'update', 'objects', 'check-integrity']) {
    modules[`./registry/${name}`] = { taskGroup: name, build() {} };
  }
  const { default: taskHandler } = await loadServerSource('server/server/internal/tasks/index.ts', modules);
  const { default: route } = await loadServerSource('server/server/api/v1/task/index.get.ts', {
    '~/server/internal/tasks': taskHandler,
    '~/server/internal/acls': aclManager,
  }, { defineWebSocketHandler: (handler) => handler });
  const peer = (headers = new Headers(), send) => {
    const frames = [];
    return {
      id: 'sample-peer', request: { headers }, frames,
      send(frame) { frames.push(frame); send?.(frame); },
      close() {},
    };
  };
  const signIn = (admin = false) => {
    state.admin = admin;
    state.session = { authenticated: { userId: 'sample-user', level: 10, requiredLevel: 10 } };
  };
  return { state, aclManager, taskHandler, route, peer, signIn };
}
