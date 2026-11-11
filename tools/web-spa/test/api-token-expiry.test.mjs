import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import test from 'node:test';
import { compiler } from '../../web-api/compiler.mjs';
import { rootDirectory as root } from '../../web-api/generate.mjs';

const now = Date.parse('2030-01-01T00:00:00.000Z');
const request = { headers: new Headers({ Authorization: 'Bearer api-token' }) };
const tokenUser = { id: 'token-user', admin: false };
const sessionUser = { id: 'session-user', admin: true };
const session = {
  authenticated: { userId: sessionUser.id, level: 2, requiredLevel: 2 },
};
const ts = compiler(root);
const source = await readFile(join(root, 'server/server/internal/acls/index.ts'), 'utf8');
const code = ts.transpileModule(source, {
  compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.CommonJS },
}).outputText;

function makeToken(mode, expiresAt) {
  return {
    id: 'token-id', token: 'api-token', mode, expiresAt,
    userId: mode === 'System' ? null : tokenUser.id,
    acls: mode === 'System' ? ['user:read'] : ['read', 'object:read'],
  };
}

function fixture({ token = null, userSession, user = tokenUser } = {}) {
  const tokenQueries = [];
  const userQueries = [];
  const prisma = {
    aPIToken: {
      async findUnique({ where }) {
        tokenQueries.push(where);
        if (!token || where.token !== token.token) return null;
        if (where.mode && !where.mode.in.includes(token.mode)) return null;
        return token;
      },
    },
    user: {
      async findUnique({ where }) {
        userQueries.push(where);
        return user?.id === where.id ? user : null;
      },
    },
  };
  const dependencies = new Map([
    ['~/prisma/client/enums', { APITokenMode: { User: 'User', System: 'System', Client: 'Client' } }],
    ['../db/database', { __esModule: true, default: prisma }],
    ['../session', { __esModule: true, default: { getSession: async () => userSession } }],
  ]);
  const exports = {};
  new Function('require', 'exports', code)((name) => {
    assert.ok(dependencies.has(name), `Unexpected module: ${name}`);
    return dependencies.get(name);
  }, exports);
  return { ...exports, tokenQueries, userQueries };
}

const paths = [
  {
    name: 'getUserIdACL', modes: ['User', 'Client'], denied: undefined,
    call: (manager) => manager.getUserIdACL(request, ['read']),
    granted: () => tokenUser.id,
    sessionResult: () => sessionUser.id,
  },
  {
    name: 'getUserACL', modes: ['User', 'Client'], denied: undefined,
    call: (manager) => manager.getUserACL(request, ['read']),
    granted: () => tokenUser,
    sessionResult: () => sessionUser,
  },
  {
    name: 'allowSystemACL', modes: ['System'], denied: false,
    call: (manager) => manager.allowSystemACL(request, ['user:read']),
    granted: () => true,
    sessionResult: () => true,
  },
  {
    name: 'allowSystemACL with no requested ACLs', modes: ['System'], denied: false,
    call: (manager) => manager.allowSystemACL(request, []),
    granted: () => true,
    sessionResult: () => true,
  },
  {
    name: 'fetchAllACLs', modes: ['User', 'System', 'Client'], denied: undefined,
    call: (manager) => manager.fetchAllACLs(request),
    granted: (token) => token.acls,
    sessionResult: ({ userACLs, systemACLs }) => [
      ...userACLs.map((acl) => `user:${acl}`),
      ...systemACLs.map((acl) => `system:${acl}`),
    ],
  },
  {
    name: 'hasACL for a user ACL', modes: ['User', 'Client'], denied: false,
    call: (manager) => manager.hasACL(request, ['user:read']),
    granted: () => true,
    sessionResult: () => true,
  },
  {
    name: 'hasACL for a system ACL', modes: ['System'], denied: false,
    call: (manager) => manager.hasACL(request, ['system:user:read']),
    granted: () => true,
    sessionResult: () => true,
  },
];

for (const path of paths) {
  for (const mode of path.modes) {
    for (const [state, expiresAt, valid] of [
      ['without an expiry', null, true],
      ['expiring in the future', new Date(now + 1), true],
      ['expired in the past', new Date(now - 1), false],
      ['expiring exactly now', new Date(now), false],
    ]) {
      test(`${path.name} handles a ${mode} token ${state}`, async (t) => {
        t.mock.method(Date, 'now', () => now);
        const token = makeToken(mode, expiresAt);
        const loaded = fixture({ token });
        assert.deepEqual(await path.call(loaded.aclManager), valid ? path.granted(token) : path.denied);
        assert.equal(loaded.tokenQueries.length, 1);
        if (!valid) {
          assert.deepEqual(await path.call(fixture().aclManager), path.denied);
          assert.deepEqual(loaded.userQueries, []);
        }
      });
    }

    test(`${path.name} keeps the session path when a ${mode} bearer token is expired`, async (t) => {
      t.mock.method(Date, 'now', () => now);
      const loaded = fixture({ token: makeToken(mode, new Date(now - 1)), userSession: session, user: sessionUser });
      assert.deepEqual(await path.call(loaded.aclManager), path.sessionResult(loaded));
      assert.deepEqual(loaded.tokenQueries, []);
    });
  }

  test(`${path.name} rejects a missing token`, async () => {
    assert.deepEqual(await path.call(fixture().aclManager), path.denied);
  });

  for (const mode of ['User', 'System', 'Client'].filter((mode) => !path.modes.includes(mode))) {
    test(`${path.name} still rejects the ${mode} token mode`, async () => {
      const token = { ...makeToken(mode, null), userId: tokenUser.id, acls: ['read', 'user:read'] };
      assert.deepEqual(await path.call(fixture({ token }).aclManager), path.denied);
    });
  }

  if (path.name !== 'fetchAllACLs' && path.name !== 'allowSystemACL with no requested ACLs') {
    test(`${path.name} still rejects tokens without a matching ACL`, async () => {
      const token = { ...makeToken(path.modes[0], null), acls: [] };
      assert.deepEqual(await path.call(fixture({ token }).aclManager), path.denied);
    });
  }
}

test('an API token stops authorizing requests when its expiry is reached', async (t) => {
  let currentTime = now;
  t.mock.method(Date, 'now', () => currentTime);
  const loaded = fixture({ token: makeToken('User', new Date(now + 1)) });
  assert.equal(await loaded.aclManager.getUserIdACL(request, ['read']), tokenUser.id);
  currentTime++;
  assert.equal(await loaded.aclManager.getUserIdACL(request, ['read']), undefined);
  assert.equal(loaded.tokenQueries.length, 2);
});

test('an expired user token without a user ID is treated as missing without throwing', async (t) => {
  t.mock.method(Date, 'now', () => now);
  const token = { ...makeToken('User', new Date(now)), userId: null };
  const loaded = fixture({ token });
  assert.equal(await loaded.aclManager.getUserIdACL(request, ['read']), undefined);
  assert.equal(await loaded.aclManager.getUserACL(request, ['read']), undefined);
  assert.deepEqual(loaded.userQueries, []);
});
