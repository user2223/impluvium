import assert from 'node:assert/strict';
import * as crypto from 'node:crypto';
import path from 'node:path';
import test from 'node:test';
import { loadServerSource, taskSocketFixture } from './server-fixture.mjs';

test('version imports create separate notification IDs and reuse the ID for the same source version', async () => {
  const rows = new Map();
  const deliveries = [];
  const prisma = {
    user: { findMany: async () => [{ id: 'sample-admin' }] },
    notification: { upsert: async ({ where, create, update }) => {
      const key = JSON.stringify(where.userId_nonce);
      const old = rows.get(key);
      const row = old ? { ...old, ...update } : { ...create, id: crypto.randomUUID() };
      rows.set(key, row);
      deliveries.push(row);
      return row;
    } },
    game: { findUnique: async () => ({ mName: 'Sample game', libraryId: 'sample-library', libraryPath: 'sample-game', type: 'Game' }) },
    gameVersion: { findFirst: async () => null, create: async () => ({ versionId: 'sample-version' }) },
    unimportedGameVersion: { findUnique: async () => ({ manifest: {}, fileList: [] }), delete: async () => {} },
  };
  const { default: notifications } = await loadServerSource('server/server/internal/notifications/index.ts', {
    '../db/database': prisma,
  });
  const pending = [];
  const { default: library } = await loadServerSource('server/server/internal/library/index.ts', {
    path,
    '../db/database': prisma,
    'fast-fuzzy': {},
    '../tasks': { create: async (task) => task.run({ progress() {}, logger: { info() {}, warn() {} } }) },
    '../notifications': { systemPush: (notification) => { pending.push(notifications.systemPush(notification)); } },
    './provider': { GameNotFoundError: class extends Error {} },
    '../logging': { logger: {} },
    'node:crypto': crypto,
    '~/server/internal/gamesize': { getVersionSize: async () => 0 },
    '~/prisma/client/enums': { GameType: { Game: 'Game', Dependency: 'Dependency', Emulator: 'Emulator' } },
    './manifest/utils': { castManifest: (manifest) => manifest },
    shescape: { Shescape: class {} },
  });
  library.addLibrary({
    id: () => 'sample-library',
    generateDropletManifest: async () => ({}),
    versionReaddir: async () => [],
  });
  const gameId = crypto.randomUUID();
  const first = { type: 'local', identifier: 'release/a-b %?#ü', name: 'First' };
  const second = { ...first, identifier: 'release/a-b %?#ü-next', name: 'Second' };
  const metadata = { delta: false, onlySetup: false, setups: [], launches: [{ name: 'Play', launch: 'game', platform: 'Linux' }] };
  for (const [id, version] of [
    [gameId, first], [gameId, second], [gameId, { ...first, name: 'Renamed first' }],
    [gameId, { ...first, type: 'depot' }], [crypto.randomUUID(), first],
  ]) {
    await library.importVersion(id, version, metadata);
    await Promise.all(pending.splice(0));
  }
  assert.notEqual(deliveries[0].id, deliveries[1].id, 'Different versions of one game must not merge');
  assert.equal(deliveries[0].id, deliveries[2].id, 'Re-importing the same source updates its notification');
  assert.match(deliveries[2].title, /Renamed first/);
  assert.notEqual(deliveries[0].id, deliveries[3].id, 'Source types have separate identities');
  assert.notEqual(deliveries[0].id, deliveries[4].id, 'Games have separate identities');
  assert.equal(rows.size, 4);
});

test('task sockets reject missing or incomplete authentication at open and on later messages before looking up tasks', async () => {
  const { state, route, peer, signIn, taskHandler } = await taskSocketFixture();
  for (const session of [undefined, {}, { authenticated: { level: 10, requiredLevel: 20 } }]) {
    state.session = session;
    const socket = peer();
    await route.open(socket);
    assert.deepEqual(socket.frames, ['unauthenticated']);
    await route.message(socket, { text: () => 'connect/sample-task' });
    assert.deepEqual(socket.frames, ['unauthenticated', 'unauthenticated']);
  }
  assert.equal(state.taskQueries, 0);

  signIn(true);
  const socket = peer();
  await route.open(socket);
  assert.deepEqual(socket.frames, ['connect']);
  // Keep an active subscription to verify that authentication loss removes it.
  const task = { ...state.task, clients: new Map() };
  taskHandler.taskPool.set(task.id, task);
  await route.message(socket, { text: () => `connect/${task.id}` });
  assert.equal(task.clients.has(socket.id), true);
  assert.equal(taskHandler.clientRegistry.has(socket.id), true);
  socket.frames.length = 0;
  state.session = undefined; // The session provider no longer returns an ended or expired session.
  await route.message(socket, { text: () => 'connect/missing-task' });
  assert.deepEqual(socket.frames, ['unauthenticated']);
  assert.equal(state.taskQueries, 0);
  assert.equal(task.clients.has(socket.id), false);
  assert.equal(taskHandler.clientRegistry.has(socket.id), false);
  await route.close(socket);

  signIn();
  const incomplete = peer();
  await route.open(incomplete);
  state.session.authenticated.requiredLevel = 20;
  await route.message(incomplete, { text: () => 'unsupported-message' });
  assert.equal(incomplete.frames.at(-1), 'unauthenticated', 'Every later message rechecks authentication');
  assert.equal(state.taskQueries, 0);
  await route.close(incomplete);
});

test('authenticated task ACL denials remain indistinguishable from nonexistent tasks', async () => {
  const { route, peer, signIn } = await taskSocketFixture();
  signIn();
  const socket = peer();
  await route.open(socket);
  assert.deepEqual(socket.frames, ['connect']);
  for (const id of ['sample-task', 'missing-task']) {
    await route.message(socket, { text: () => `connect/${id}` });
    assert.equal(socket.frames.at(-1), `error/${id}/Unknown task/impluvium couldn't find the task you're looking for.`);
  }
  await route.close(socket);
});

test('task subscriptions arriving during the opening credential check still authenticate', async () => {
  const { route, peer, signIn } = await taskSocketFixture();
  signIn(true);
  const socket = peer();
  const opening = route.open(socket);
  await route.message(socket, { text: () => 'connect/sample-task' });
  await opening;
  assert.ok(!socket.frames.includes('unauthenticated'));
  assert.ok(socket.frames.includes('connect'));
  assert.equal(JSON.parse(socket.frames.find((frame) => frame.startsWith('{'))).id, 'sample-task');
  await route.close(socket);
});

test('expired scoped task tokens receive only an authentication frame without unrelated task IDs', async () => {
  const { state, route, peer, taskHandler } = await taskSocketFixture();
  const token = { mode: 'System', acls: ['task:read'], expiresAt: null };
  state.tokens.set('scoped-token', token);
  const permitted = { ...state.task, clients: new Map() };
  const forbidden = { ...state.task, id: 'forbidden-task', acls: ['system:maintenance:read'], clients: new Map([['other-peer', true]]) };
  taskHandler.taskPool.set(permitted.id, permitted);
  taskHandler.taskPool.set(forbidden.id, forbidden);
  const socket = peer(new Headers({ Authorization: 'Bearer scoped-token' }));
  await route.open(socket);
  await route.message(socket, { text: () => `connect/${permitted.id}` });
  const expected = ['connect', JSON.stringify({
    id: permitted.id, name: permitted.name, success: false, log: [], progress: 0, actions: [],
  })];
  assert.deepEqual(socket.frames, expected);
  assert.equal(permitted.clients.has(socket.id), true);
  token.expiresAt = new Date(0);
  await route.message(socket, { text: () => `connect/${permitted.id}` });
  expected.push('unauthenticated');
  assert.deepEqual(socket.frames, expected);
  assert.equal(permitted.clients.has(socket.id), false);
  assert.equal(taskHandler.clientRegistry.has(socket.id), false);
  assert.deepEqual([...forbidden.clients], [['other-peer', true]]);
  await route.close(socket);
  await route.close(socket);
  assert.deepEqual(socket.frames, expected, 'Repeated close cleanup must be silent');
});

test('ordinary task disconnection reports only actual subscriptions and is idempotent', async () => {
  const { state, route, peer, signIn, taskHandler } = await taskSocketFixture();
  signIn(true);
  const tasks = ['sample-task', 'second-task', 'unsubscribed-task'].map((id) => ({ ...state.task, id, clients: new Map() }));
  for (const task of tasks) taskHandler.taskPool.set(task.id, task);
  const socket = peer();
  await route.open(socket);
  for (const task of tasks.slice(0, 2)) {
    await route.message(socket, { text: () => `connect/${task.id}` });
  }
  socket.frames.length = 0;
  assert.equal(taskHandler.disconnect(socket.id, 'unsubscribed-task'), false);
  assert.deepEqual(socket.frames, []);
  taskHandler.disconnectAll(socket.id);
  assert.deepEqual(socket.frames, ['disconnect/sample-task', 'disconnect/second-task']);
  taskHandler.disconnectAll(socket.id);
  assert.equal(taskHandler.disconnect(socket.id, 'sample-task'), false);
  assert.deepEqual(socket.frames, ['disconnect/sample-task', 'disconnect/second-task']);
  assert.equal(taskHandler.clientRegistry.has(socket.id), false);
  await route.close(socket);
});

test('socket close silently clears subscriptions even without a stored request', async () => {
  const { state, route, peer, taskHandler } = await taskSocketFixture();
  const socket = peer();
  const task = { ...state.task, clients: new Map([[socket.id, true]]) };
  taskHandler.taskPool.set(task.id, task);
  taskHandler.clientRegistry.set(socket.id, socket);
  await route.close(socket);
  await route.close(socket);
  assert.equal(task.clients.has(socket.id), false);
  assert.equal(taskHandler.clientRegistry.has(socket.id), false);
  assert.deepEqual(socket.frames, []);
});

function deferred() {
  let resolve;
  const promise = new Promise((accept) => { resolve = accept; });
  return { promise, resolve };
}

for (const query of ['task', 'ACL']) {
  for (const invalidation of ['message authentication failure', 'open authentication failure', 'open without a request', 'close']) {
    test(`a pending ${query} query cannot restore a task subscription after ${invalidation}`, async () => {
      const { state, route, peer, signIn, taskHandler, aclManager } = await taskSocketFixture();
      signIn(true);
      const pendingTask = { ...state.task, clients: new Map() };
      const forbidden = { ...state.task, id: 'forbidden-task', acls: ['system:maintenance:read'], clients: new Map() };
      taskHandler.taskPool.set(forbidden.id, forbidden);
      if (query === 'ACL') taskHandler.taskPool.set(pendingTask.id, pendingTask);
      const reached = deferred();
      const release = deferred();
      let aclQueries = 0;
      const hasACL = aclManager.hasACL.bind(aclManager);
      aclManager.hasACL = async (...args) => {
        aclQueries++;
        const allowed = await hasACL(...args);
        if (query === 'ACL') {
          reached.resolve();
          await release.promise;
        }
        return allowed;
      };
      if (query === 'task') {
        state.findTask = async () => {
          reached.resolve();
          await release.promise;
          return state.task;
        };
      }
      const socket = peer();
      await route.open(socket);
      const connecting = route.message(socket, { text: () => `connect/${pendingTask.id}` });
      await reached.promise;
      state.session = undefined;
      if (invalidation === 'message authentication failure') {
        await route.message(socket, { text: () => 'connect/forbidden-task' });
      } else if (invalidation === 'open authentication failure') {
        await route.open(socket);
      } else if (invalidation === 'open without a request') {
        socket.request = undefined;
        await route.open(socket);
      } else {
        await route.close(socket);
      }
      const expected = invalidation === 'close' ? ['connect'] : ['connect', 'unauthenticated'];
      assert.deepEqual(socket.frames, expected);
      release.resolve();
      await connecting;
      assert.deepEqual(socket.frames, expected, 'A stale subscription must send neither a snapshot nor an error');
      assert.equal(aclQueries, query === 'ACL' ? 1 : 0);
      assert.equal(pendingTask.clients.has(socket.id), false);
      assert.equal(forbidden.clients.has(socket.id), false);
      assert.equal(taskHandler.clientRegistry.has(socket.id), false);
      await route.close(socket);
      await route.close(socket);
      assert.deepEqual(socket.frames, expected);
    });
  }
}

test('task sockets retain API token access and distinguish absent or expired tokens from missing ACLs', async () => {
  const { state, route, peer } = await taskSocketFixture();
  for (const mode of ['System', 'User', 'Client']) {
    state.tokens.set('sample-token', { mode, userId: 'sample-user', acls: ['task:read'], expiresAt: new Date(Date.now() + 60000) });
    state.task.acls = [mode === 'System' ? 'system:task:read' : 'user:read'];
    if (mode !== 'System') state.tokens.get('sample-token').acls = ['read'];
    const socket = peer(new Headers({ Authorization: 'Bearer sample-token' }));
    await route.open(socket);
    await route.message(socket, { text: () => 'connect/sample-task' });
    assert.equal(JSON.parse(socket.frames.at(-1)).id, 'sample-task');
    state.tokens.get('sample-token').acls = [];
    await route.message(socket, { text: () => 'connect/sample-task' });
    assert.match(socket.frames.at(-1), /^error\/sample-task\/Unknown task\//);
    state.tokens.delete('sample-token');
    await route.message(socket, { text: () => 'connect/sample-task' });
    assert.equal(socket.frames.at(-1), 'unauthenticated');
    await route.close(socket);
  }
  state.tokens.set('expired-token', { mode: 'System', acls: ['task:read'], expiresAt: new Date(0) });
  const expired = peer(new Headers({ Authorization: 'Bearer expired-token' }));
  await route.open(expired);
  assert.deepEqual(expired.frames, ['unauthenticated']);
});
