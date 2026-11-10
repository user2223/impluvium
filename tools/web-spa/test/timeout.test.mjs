import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { join } from 'node:path';
import test from 'node:test';
import { rootDirectory as root } from '../../web-api/generate.mjs';
import { createBrowserTransport } from '../../../server/composables/browser-transport.ts';

test('the transport deadline aborts a hanging real ofetch dispatch with a supplied signal', async () => {
  const require = createRequire(join(root, 'server/package.json'));
  const { ofetch } = require('ofetch');
  const set = globalThis.setTimeout;
  const clear = globalThis.clearTimeout;
  const timers = new Map();
  let id = 0;
  globalThis.setTimeout = (callback, delay) => { const key = ++id; timers.set(key, { callback, delay }); return key; };
  globalThis.clearTimeout = (key) => { timers.delete(key); };
  let dispatches = 0;
  let signal;
  try {
    const fetch = ofetch.create({}, {
      fetch: (_url, options) => {
        dispatches++;
        signal = options.signal;
        return new Promise((_, reject) => {
          signal.addEventListener('abort', () => reject(signal.reason), { once: true });
        });
      },
    });
    const transport = createBrowserTransport((url, options) => fetch.raw(url, options));
    const pending = transport('/api/v1/user', { timeout: 15000 });
    const failure = assert.rejects(pending, { name: 'TimeoutError' });
    assert.equal(dispatches, 1);
    assert.equal(signal.aborted, false);
    assert.equal(timers.size, 1, 'The transport must own a timeout despite ofetch receiving a signal');
    const [key, timer] = [...timers][0];
    assert.equal(timer.delay, 15000);
    timers.delete(key);
    timer.callback();
    await failure;
    assert.equal(signal.aborted, true);
    await Promise.resolve();
    await Promise.resolve();
    assert.equal(dispatches, 1, 'The expired request must not be retried by ofetch');
    assert.equal(timers.size, 0);
  } finally {
    globalThis.setTimeout = set;
    globalThis.clearTimeout = clear;
  }
});
