import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import test from 'node:test';
import { compiler } from '../../web-api/compiler.mjs';
import { rootDirectory as root } from '../../web-api/generate.mjs';
import { advanceSessionContext, confirmSessionContext, sessionContext } from '../../../server/composables/session-context.ts';

test('signin checks its generation after fetch resolution before confirming or navigating', async () => {
  const originals = new Map();
  const install = (name, value) => {
    originals.set(name, Object.getOwnPropertyDescriptor(globalThis, name));
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value });
  };
  const user = { value: undefined };
  const navigations = [];
  const requests = [];
  install('useState', () => user);
  install('useRoute', () => ({ query: { redirect: '/admin/users' } }));
  install('useRouter', () => ({ push: async (path) => { navigations.push(path); } }));
  install('$apiFetch', (key) => {
    assert.equal(key, 'GET /api/v1/user');
    return new Promise((resolve) => requests.push(resolve));
  });
  try {
    const ts = compiler(root);
    const source = await readFile(join(root, 'server/composables/user.ts'), 'utf8');
    const code = ts.transpileModule(source, {
      compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext },
    }).outputText.replaceAll('"./session-context"', JSON.stringify(pathToFileURL(join(root, 'server/composables/session-context.ts')).href));
    const { completeSignin } = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
    for (const replacement of [null, { id: 'current-user', admin: true }]) {
      const old = completeSignin();
      // The transport has already fulfilled its promise. Change context before
      // the awaiting signin continuation gets its turn in the microtask queue.
      requests.at(-1)({ id: 'obsolete-user', admin: false });
      advanceSessionContext();
      confirmSessionContext(replacement);
      user.value = replacement;
      const generation = sessionContext().generation;
      await old;
      assert.strictEqual(user.value, replacement);
      assert.equal(sessionContext().session?.id, replacement?.id);
      assert.equal(sessionContext().generation, generation);
      assert.deepEqual(navigations, []);
    }
    const current = completeSignin();
    requests.at(-1)({ id: 'signed-in-user', admin: true });
    await current;
    assert.equal(user.value.id, 'signed-in-user');
    assert.equal(sessionContext().session.id, 'signed-in-user');
    assert.deepEqual(navigations, ['/admin/users']);
  } finally {
    advanceSessionContext();
    for (const [name, descriptor] of originals) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else delete globalThis[name];
    }
  }
});

async function startupFixture(run) {
  const originals = new Map();
  const install = (name, value) => {
    originals.set(name, Object.getOwnPropertyDescriptor(globalThis, name));
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value });
  };
  const user = { value: undefined };
  const requests = [];
  const navigations = [];
  install('useState', () => user);
  install('useRoute', () => ({ query: {} }));
  install('useRouter', () => ({ push: async (path) => { navigations.push(path); } }));
  install('useError', () => ({ value: undefined }));
  install('defineNuxtRouteMiddleware', (middleware) => middleware);
  install('navigateTo', (target) => { navigations.push(target); });
  install('$apiFetch', (key, options) => {
    assert.equal(key, 'GET /api/v1/user');
    assert.deepEqual(options, { timeout: 15000 });
    return new Promise((resolve, reject) => requests.push({ resolve, reject }));
  });
  try {
    advanceSessionContext();
    const ts = compiler(root);
    const load = async (file) => {
      const source = await readFile(join(root, file), 'utf8');
      const code = ts.transpileModule(source, {
        compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext },
      }).outputText.replaceAll('"./session-context"', JSON.stringify(pathToFileURL(join(root, 'server/composables/session-context.ts')).href));
      return import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
    };
    const helpers = await load('server/composables/user.ts');
    install('useUser', helpers.useUser);
    install('initializeUser', helpers.initializeUser);
    const { default: requireUser } = await load('server/middleware/require-user.global.ts');
    await run({ ...helpers, requireUser, user, requests, navigations });
  } finally {
    advanceSessionContext();
    for (const [name, descriptor] of originals) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else delete globalThis[name];
    }
  }
}

test('the route guard and app share one startup request, including after session confirmation', async () => {
  await startupFixture(async ({ initializeUser, updateUser, completeSignin, requireUser, user, requests, navigations }) => {
    const generation = sessionContext().generation;
    const route = requireUser({ fullPath: '/admin/users' });
    const startup = initializeUser();
    assert.strictEqual(initializeUser(), startup, 'Concurrent startup readers must share their promise');
    assert.equal(requests.length, 1);
    const initialUser = { id: 'initial-admin', admin: true };
    requests[0].resolve(initialUser);
    await route;
    assert.strictEqual(user.value, initialUser);
    assert.equal(sessionContext().generation, generation + 1, 'Confirmation must retain its existing generation change');
    // The real app starts after the initial route guard has already finished.
    assert.strictEqual(initializeUser(), startup, 'Sequential startup must reuse the confirmed decision');
    await initializeUser();
    await requireUser({ fullPath: '/admin/users' });
    assert.equal(requests.length, 1);
    assert.deepEqual(navigations, []);

    const refresh = updateUser();
    assert.equal(requests.length, 2, 'An explicit refresh must still fetch new profile data');
    requests[1].resolve({ ...initialUser, displayName: 'Updated profile' });
    await refresh;
    assert.equal(user.value.displayName, 'Updated profile');
    assert.equal(sessionContext().generation, generation + 1);

    const signin = completeSignin();
    assert.equal(requests.length, 3, 'Signin must still fetch and confirm its own user');
    requests[2].resolve({ id: 'next-admin', admin: true });
    await signin;
    assert.equal(user.value.id, 'next-admin');
    assert.deepEqual(navigations, ['/']);
  });
});

test('anonymous startup shares its null decision and preserves the explicit refresh short circuit', async () => {
  await startupFixture(async ({ initializeUser, updateUser, user, requests }) => {
    const generation = sessionContext().generation;
    const startup = initializeUser();
    assert.strictEqual(initializeUser(), startup);
    requests[0].resolve(null);
    await startup;
    assert.strictEqual(initializeUser(), startup);
    await initializeUser();
    await updateUser();
    assert.equal(requests.length, 1);
    assert.equal(user.value, null);
    assert.equal(sessionContext().generation, generation);
  });
});

test('startup readers receive the same timeout failure without a second request', async () => {
  await startupFixture(async ({ initializeUser, updateUser, requireUser, user, requests }) => {
    const generation = sessionContext().generation;
    const error = new DOMException('The request timed out', 'TimeoutError');
    const route = requireUser({ fullPath: '/admin/users' });
    const startup = initializeUser();
    const failures = [route, startup].map((promise) => assert.rejects(promise, (value) => value === error));
    requests[0].reject(error);
    await Promise.all(failures);
    assert.strictEqual(initializeUser(), startup);
    await assert.rejects(initializeUser(), (value) => value === error);
    assert.equal(requests.length, 1);
    assert.equal(user.value, undefined);
    assert.equal(sessionContext().generation, generation);

    const explicit = updateUser();
    assert.equal(requests.length, 2, 'A failed startup must not suppress explicit refreshes');
    requests[1].resolve({ id: 'recovered-user', admin: false });
    await explicit;
    assert.equal(user.value.id, 'recovered-user');
  });
});

test('a context change replaces the startup promise and ignores obsolete completion', async () => {
  await startupFixture(async ({ initializeUser, user, requests }) => {
    for (const completion of ['response', 'error']) {
      advanceSessionContext();
      user.value = undefined;
      const old = initializeUser();
      const obsolete = requests.at(-1);
      const oldCompletion = completion === 'error' ? assert.rejects(old, /Obsolete request/) : old;
      advanceSessionContext();
      const current = initializeUser();
      assert.notStrictEqual(current, old);
      const currentUser = { id: `current-${completion}`, admin: false };
      requests.at(-1).resolve(currentUser);
      await current;
      const generation = sessionContext().generation;
      const count = requests.length;
      if (completion === 'response') obsolete.resolve({ id: 'obsolete-user', admin: true });
      else obsolete.reject(new Error('Obsolete request'));
      await oldCompletion;
      assert.strictEqual(user.value, currentUser);
      assert.equal(sessionContext().generation, generation);
      assert.strictEqual(initializeUser(), current);
      await initializeUser();
      assert.equal(requests.length, count);
    }
  });
});
