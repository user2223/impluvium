import assert from 'node:assert/strict';
import test from 'node:test';
import { loadWrapper } from '../../web-api/test/wrapper.mjs';
import { advanceSessionContext } from '../../../server/composables/session-context.ts';

test('the typed HTTP wrapper passes retry: false through to ofetch', async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, '$fetch');
  const calls = [];
  Object.defineProperty(globalThis, '$fetch', { configurable: true, value: {
    raw: async (url, options) => {
      calls.push({ url, options });
      return { status: 200, _data: [] };
    },
  } });
  try {
    const { $apiFetch } = await loadWrapper();
    await $apiFetch('GET /api/v1/notifications', { retry: false });
    assert.equal(calls.length, 1);
    assert.equal(calls[0].url, '/api/v1/notifications');
    assert.equal(calls[0].options.retry, false);
  } finally {
    if (original) Object.defineProperty(globalThis, '$fetch', original);
    else delete globalThis.$fetch;
  }
});

test('failTitle handles foreign error bodies and suppresses stale or aborted errors', async () => {
  const originals = new Map();
  const install = (name, value) => {
    if (!originals.has(name)) originals.set(name, Object.getOwnPropertyDescriptor(globalThis, name));
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value });
  };
  const notices = [];
  install('ModalType', { Notification: 'notification' });
  install('createModal', (type, data) => { notices.push({ type, data }); });
  try {
    const { $apiFetch } = await loadWrapper();
    for (const data of [{ message: 'Synthetic validation error' }, null, undefined,
      '<html>Proxy failure</html>', 'plain text', '{broken-json', { message: 123 }]) {
      notices.length = 0;
      install('$fetch', { raw: async () => { throw Object.assign(new Error('Synthetic'), { data }); } });
      await assert.rejects($apiFetch('GET /api/v1', { failTitle: 'Synthetic title' }), /Synthetic/);
      assert.equal(notices.length, 1);
      assert.equal(notices[0].data.title, 'Synthetic title');
      assert.equal(notices[0].data.description, typeof data?.message === 'string'
        ? data.message : 'The request could not be completed. Please try again.');
    }
    notices.length = 0;
    let fail;
    install('$fetch', { raw: () => new Promise((_, reject) => { fail = reject; }) });
    const old = $apiFetch('GET /api/v1', { failTitle: 'Old session' });
    const rejected = assert.rejects(old, { name: 'AbortError' });
    advanceSessionContext();
    fail(new Error('Late old failure'));
    await rejected;
    assert.deepEqual(notices, []);

    const controller = new AbortController();
    controller.abort();
    await assert.rejects($apiFetch('GET /api/v1', { signal: controller.signal, failTitle: 'Cancelled' }), { name: 'AbortError' });
    assert.deepEqual(notices, []);
  } finally {
    for (const [name, descriptor] of originals) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else delete globalThis[name];
    }
  }
});
