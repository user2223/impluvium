import assert from 'node:assert/strict';
import test from 'node:test';
import { databaseSessionFixture, loadServerSource, taskSocketFixture } from './server-fixture.mjs';

test('database session reads return the same authenticated payload and token before and after caching', async () => {
  const { provider, state, cache, token, session } = await databaseSessionFixture();
  const expected = { ...session, token };
  assert.equal(await cache.get(token), null);
  const first = await provider.getSession(token);
  const second = await provider.getSession(token);
  assert.deepEqual(first, expected);
  assert.deepEqual(second, expected);
  assert.deepEqual(await cache.get(token), expected);
  assert.equal(state.reads, 1, 'The second read must use the populated cache');
  assert.deepEqual(await provider.setSession(token, session), expected);
  assert.deepEqual(await provider.getSession(token), expected);
  assert.equal(state.reads, 1);
});

test('database session reads ignore row-shaped entries in the old cache namespace', async () => {
  const { provider, state, cache, cacheFor, token, session } = await databaseSessionFixture();
  const legacyCache = cacheFor('DBSession');
  await legacyCache.set(token, state.row);
  const legacyRow = await legacyCache.get(token);
  assert.equal(legacyRow.authenticated, undefined);
  assert.deepEqual(legacyRow.data.authenticated, session.authenticated);
  assert.equal(await cache.get(token), null);
  const expected = { ...session, token };
  assert.deepEqual(await provider.getSession(token), expected);
  assert.deepEqual(await provider.getSession(token), expected);
  assert.deepEqual(await cache.get(token), expected);
  assert.equal(state.reads, 1, 'The legacy entry must not prevent a fresh database read');
  assert.deepEqual(await legacyCache.get(token), legacyRow);
});

test('session search attaches the database token so OIDC search results can be signed out', async () => {
  const { handler, state, cache, token, session } = await databaseSessionFixture();
  const oidc = { iss: 'sample-issuer', sid: 'sample-session-id' };
  session.oidc = oidc;
  assert.equal(Object.hasOwn(state.row.data, 'token'), false);
  const request = { headers: new Headers({ Cookie: `drop-token=${token}` }) };
  assert.deepEqual(await handler.getSession(request), { ...session, token });
  const sessions = await handler.searchSessions({ oidc });
  assert.deepEqual(state.searches, [{ AND: [
    { data: { path: ['oidc', 'iss'], equals: oidc.iss } },
    { data: { path: ['oidc', 'sid'], equals: oidc.sid } },
  ] }]);
  assert.deepEqual(sessions, [{ ...session, token }]);
  await Promise.all(sessions.map((value) => handler.signoutByToken(value.token)));
  assert.equal(state.row, null);
  assert.equal(await cache.get(token), null);
  assert.equal(await handler.getSession(request), undefined);
});

test('consecutive user HTTP requests keep cookie authentication after a cold database session cache', async () => {
  const { handler, state, token } = await databaseSessionFixture();
  const server = await taskSocketFixture({ sessionHandler: handler });
  server.state.admin = true;
  const { default: route } = await loadServerSource('server/server/api/v1/user/index.get.ts', {
    '~/server/internal/acls': server.aclManager,
  }, { defineEventHandler: (callback) => callback });
  const request = { headers: new Headers({ Cookie: `drop-token=${token}` }) };
  const expected = { id: 'sample-user', admin: true };
  assert.deepEqual(await route(request), expected);
  assert.deepEqual(await route(request), expected);
  assert.equal(state.reads, 1);
});

test('a task socket keeps cookie authentication from a cold session cache through later messages', async () => {
  const { handler, state, token } = await databaseSessionFixture();
  const server = await taskSocketFixture({ sessionHandler: handler });
  server.state.admin = true;
  const socket = server.peer(new Headers({ Cookie: `drop-token=${token}` }));
  const snapshot = JSON.stringify({ id: 'sample-task', name: 'Sample task', success: false, log: [], progress: 0, actions: [] });
  await server.route.open(socket);
  assert.deepEqual(socket.frames, ['connect']);
  await server.route.message(socket, { text: () => 'connect/sample-task' });
  await server.route.message(socket, { text: () => 'connect/sample-task' });
  assert.deepEqual(socket.frames, ['connect', snapshot, snapshot]);
  assert.equal(state.reads, 1, 'Open reads the database; message authentication and task ACL checks use the cache');
  await handler.signoutByToken(token);
  await server.route.message(socket, { text: () => 'connect/sample-task' });
  assert.deepEqual(socket.frames, ['connect', snapshot, snapshot, 'unauthenticated']);
  await server.route.close(socket);
});
