import assert from 'node:assert/strict';
import test from 'node:test';
import { createBrowserTransport } from '../../../server/composables/browser-transport.ts';
import { advanceSessionContext, sessionContext } from '../../../server/composables/session-context.ts';
import { binaryValues, jsonResponses, opaqueResponses, ordinaryBodies, streamingBodies, unsupportedBody } from '../../ci/fixtures/fetch-values.mjs';

// Exercise the browser transport with the existing body/response vectors.

function fixture() {
  const calls = [];
  const transport = createBrowserTransport((url, options) => new Promise((resolve, reject) => {
    calls.push({ url, options, resolve: (value, status = 200) => resolve({ status, _data: value }), reject });
  }));
  return { calls, transport };
}

test('only pending identical GETs share a request; no completed response is cached', async () => {
  const { calls, transport } = fixture();
  const first = transport('/api/v1', { query: { b: 2, a: 1 } });
  const second = transport('/api/v1', { query: { a: 1, b: 2 } });
  assert.equal(calls.length, 1);
  calls[0].resolve({ values: [1] });
  const [a, b] = await Promise.all([first, second]);
  a.values.push(2);
  assert.deepEqual(b, { values: [1] });
  const third = transport('/api/v1', { query: { a: 1, b: 2 } });
  assert.equal(calls.length, 2);
  calls[1].resolve(null);
  await third;
});

test('route, query and response modes distinguish pending GETs', async () => {
  const { calls, transport } = fixture();
  const reads = [transport('/api/v1'), transport('/api/v1/user'),
    transport('/api/v1', { query: { q: 'different' } }), transport('/api/v1', { responseType: 'text' })];
  assert.equal(calls.length, 4);
  calls.forEach((call, index) => call.resolve(index));
  assert.deepEqual(await Promise.all(reads), [0, 1, 2, 3]);
});

for (const url of ['/api/v1/admin', '/api/v1/admin/auth']) {
  test(`setup requests with explicit headers stay separate at ${url}`, async () => {
    const { calls, transport } = fixture();
    const headers = new Headers({ Authorization: 'Bearer synthetic-a' });
    const tuples = [['Authorization', 'Bearer synthetic-b']];
    const requests = [transport(url, { headers }), transport(url, { headers }),
      transport(url, { headers: tuples }), transport(url), transport(url, { headers: {} }),
      transport(url, { headers: undefined })];
    assert.equal(calls.length, 6);
    calls.forEach((call, index) => call.resolve({ index }));
    assert.deepEqual(await Promise.all(requests), requests.map((_, index) => ({ index })));
    assert.equal(headers.get('Authorization'), 'Bearer synthetic-a');
    assert.deepEqual(tuples, [['Authorization', 'Bearer synthetic-b']]);
    assert.ok(calls.every((call) => call.options.credentials === 'same-origin'));
  });
}

test('one reader aborts without cancelling another reader of the same GET', async () => {
  const { calls, transport } = fixture();
  const controller = new AbortController();
  const first = transport('/api/v1', { signal: controller.signal });
  const second = transport('/api/v1');
  const rejection = assert.rejects(first, { name: 'AbortError' });
  controller.abort();
  assert.equal(calls[0].options.signal.aborted, false);
  calls[0].resolve({ current: true });
  await rejection;
  assert.deepEqual(await second, { current: true });
});

for (const completion of ['response', 'error']) {
  test(`a late old-session ${completion} cannot overwrite the new context`, async () => {
    const { calls, transport } = fixture();
    const first = transport('/api/v1');
    const rejection = assert.rejects(first, { name: 'AbortError' });
    advanceSessionContext();
    assert.equal(calls[0].options.signal.aborted, true);
    const second = transport('/api/v1');
    assert.equal(calls.length, 2);
    calls[1].resolve({ user: 'synthetic-b' });
    assert.deepEqual(await second, { user: 'synthetic-b' });
    if (completion === 'response') calls[0].resolve({ user: 'synthetic-a' });
    else calls[0].reject(new Error('Synthetic old failure'));
    await rejection;
  });
}

test('mutations and HEADs never share, and cancellation reaches non-shared requests', async () => {
  const { calls, transport } = fixture();
  const controllers = [];
  const reads = ['POST', 'PATCH', 'DELETE', 'HEAD'].flatMap((method) => [0, 1].map(() => {
    const controller = new AbortController();
    controllers.push(controller);
    return transport('/api/v1/collection', { method, signal: controller.signal });
  }));
  assert.equal(calls.length, 8);
  const aborted = assert.rejects(reads[0], { name: 'AbortError' });
  controllers[0].abort();
  assert.equal(calls[0].options.signal.aborted, true);
  calls.forEach((call, index) => call.resolve(index));
  await aborted;
  assert.deepEqual(await Promise.all(reads.slice(1)), [1, 2, 3, 4, 5, 6, 7]);
});

test('a failed shared GET leaves no failure cache', async () => {
  const { calls, transport } = fixture();
  const a = transport('/api/v1');
  const b = transport('/api/v1');
  const failed = Promise.all([assert.rejects(a, /Synthetic/), assert.rejects(b, /Synthetic/)]);
  calls[0].reject(new Error('Synthetic'));
  await failed;
  const retry = transport('/api/v1');
  assert.equal(calls.length, 2);
  calls[1].resolve(0);
  assert.equal(await retry, 0);
});

for (const value of jsonResponses) {
  test(`JSON and empty response remain valid: ${JSON.stringify(value)}`, async () => {
    const { calls, transport } = fixture();
    const pending = transport('/api/v1');
    calls[0].resolve(value);
    assert.deepEqual(await pending, value === undefined || value === '' ? null : value);
  });
}

test('HTTP 204 always yields null', async () => {
  const { calls, transport } = fixture();
  const request = transport('/api/v1');
  calls[0].resolve(undefined, 204);
  assert.equal(await request, null);
});

test('multipart and URL-encoded mutations preserve bytes and ordered fields without sharing', async () => {
  for (const kind of ['multipart', 'url-encoded']) {
    const { calls, transport } = fixture();
    const bodies = ['file-bytes-AAAA', 'file-bytes-BBBB'].map((contents) => {
      if (kind === 'url-encoded') return new URLSearchParams([['tag', 'first'], ['tag', 'second']]);
      const form = new FormData();
      form.append('tag', 'first');
      form.append('tag', 'second');
      form.append('upload', new Blob([contents]), 'sample.bin');
      return form;
    });
    const requests = bodies.map((body) => transport('/api/v1/upload', { method: 'POST', body }));
    assert.equal(calls.length, 2);
    for (const [index, call] of calls.entries()) {
      assert.strictEqual(call.options.body, bodies[index]);
      assert.deepEqual(call.options.body.getAll('tag'), ['first', 'second']);
      if (kind === 'multipart') {
        assert.equal(call.options.body.get('upload').name, 'sample.bin');
        assert.equal(await call.options.body.get('upload').text(), index ? 'file-bytes-BBBB' : 'file-bytes-AAAA');
      }
      call.resolve(index);
    }
    assert.deepEqual(await Promise.all(requests), [0, 1]);
  }
});

test('every existing body and binary/opaque response vector passes unchanged on mutations', async () => {
  for (const { name, create } of [...ordinaryBodies, ...binaryValues, ...streamingBodies,
    { name: 'unsupported', create: unsupportedBody }]) {
    const { calls, transport } = fixture();
    const body = create();
    const options = { method: 'POST', body };
    const request = transport('/api/v1/collection', options);
    assert.strictEqual(calls[0].options.body, body, name);
    if (body instanceof ReadableStream) assert.equal(body.locked, false, name);
    assert.deepEqual(Object.keys(options), ['method', 'body']);
    calls[0].resolve({ ok: true });
    await request;
    if (body instanceof ReadableStream) await body.cancel();
    else if (typeof body?.destroy === 'function') body.destroy();
  }
  for (const { name, create } of [...binaryValues, ...opaqueResponses]) {
    const { calls, transport } = fixture();
    const value = create();
    const request = transport('/api/v1/collection', { method: 'POST' });
    calls[0].resolve(value);
    assert.strictEqual(await request, value, name);
    // Only known stream fixtures need cleanup; opaque results must not be inspected.
    if (name === 'ReadableStream') await value.cancel();
    else if (name === 'Node stream parser result') value.destroy();
  }
});

test('request URLs reject every C0 control and DEL before dispatch', async () => {
  const calls = [];
  const transport = createBrowserTransport(async (url) => {
    calls.push(url);
    return { status: 200, _data: 'accepted' };
  });
  for (const code of [...Array.from({ length: 32 }, (_, index) => index), 0x7f]) {
    const character = String.fromCharCode(code);
    for (const url of [`/api/v1/a${character}b`, `/api/v1?q=a${character}b`]) {
      await assert.rejects(transport(url), /current origin/, `U+${code.toString(16)} must be rejected`);
    }
  }
  await assert.rejects(transport('/api/v1/a\\b'), /current origin/);
  assert.deepEqual(calls, []);
  for (const character of [' ', '~', '\u0080', 'ä', '😀']) {
    const url = `/api/v1/a${character}b`;
    assert.equal(await transport(url), 'accepted');
    assert.equal(calls.at(-1), url);
  }
});

test('options stay unchanged, including credentials, query, headers and abort signal', async () => {
  const { calls, transport } = fixture();
  const controller = new AbortController();
  const options = Object.freeze({ method: 'POST', signal: controller.signal,
    headers: Object.freeze({ Authorization: 'Bearer synthetic' }),
    query: Object.freeze({ q: 'sample & ü' }), body: Object.freeze({ title: 'Sample' }) });
  const request = transport('/api/v1/collection', options);
  assert.equal(calls[0].options.signal.aborted, false);
  assert.strictEqual(options.signal, controller.signal);
  assert.strictEqual(calls[0].options.headers, options.headers);
  assert.strictEqual(calls[0].options.query, options.query);
  calls[0].resolve(null);
  await request;
  for (const url of ['https://example.invalid/api', '//example.invalid/api']) {
    await assert.rejects(transport(url), /current origin/);
  }
  assert.equal(sessionContext().signal.aborted, false);
});

test('deadlines combine with session and reader cancellation, including shared GETs', async () => {
  const set = globalThis.setTimeout;
  const clear = globalThis.clearTimeout;
  const timers = new Map();
  let id = 0;
  globalThis.setTimeout = (callback, delay) => { const key = ++id; timers.set(key, { callback, delay }); return key; };
  globalThis.clearTimeout = (key) => { timers.delete(key); };
  const fire = (key) => { const timer = timers.get(key); timers.delete(key); timer.callback(); };
  try {
    const { calls, transport } = fixture();
    const options = Object.freeze({ timeout: 15000 });
    const first = transport('/api/v1/user', options);
    const second = transport('/api/v1/user', options);
    const [firstTimer, secondTimer] = [...timers.keys()];
    assert.deepEqual([...timers.values()].map((timer) => timer.delay), [15000, 15000]);
    assert.equal(calls.length, 1);
    assert.equal(Object.hasOwn(calls[0].options, 'timeout'), false);
    const firstFailure = assert.rejects(first, { name: 'TimeoutError' });
    fire(firstTimer);
    await firstFailure;
    assert.equal(calls[0].options.signal.aborted, false, 'One deadline must not cancel the other reader');
    const secondFailure = assert.rejects(second, { name: 'TimeoutError' });
    fire(secondTimer);
    await secondFailure;
    assert.equal(calls[0].options.signal.aborted, true);
    assert.equal(timers.size, 0);
    assert.deepEqual(options, { timeout: 15000 });

    const fresh = transport('/api/v1/user', options);
    assert.equal(calls.length, 2, 'An expired shared request must not be reused');
    calls[0].resolve({ id: 'late' });
    calls[1].resolve({ id: 'current' });
    assert.deepEqual(await fresh, { id: 'current' });
    assert.equal(timers.size, 0, 'Successful requests must clear their deadlines');

    const controller = new AbortController();
    const mutation = transport('/api/v1/collection', { method: 'POST', timeout: 100, signal: controller.signal });
    const cancelled = assert.rejects(mutation, { name: 'AbortError' });
    controller.abort();
    await cancelled;
    assert.equal(calls.at(-1).options.signal.aborted, true);
    assert.equal(timers.size, 0);
    const old = transport('/api/v1', { timeout: 100 });
    const changed = assert.rejects(old, { name: 'AbortError' });
    advanceSessionContext();
    await changed;
    assert.equal(calls.at(-1).options.signal.aborted, true);
    assert.equal(timers.size, 0);

    const disabled = transport('/api/v1', { timeout: 0 });
    assert.equal(timers.size, 0);
    calls.at(-1).resolve(null);
    await disabled;
    const fail = transport('/api/v1', { timeout: 100 });
    const failed = assert.rejects(fail, /Synthetic/);
    calls.at(-1).reject(new Error('Synthetic'));
    await failed;
    assert.equal(timers.size, 0, 'Failed requests must clear their deadlines');
    const preAborted = new AbortController();
    preAborted.abort();
    const count = calls.length;
    await assert.rejects(transport('/api/v1', { timeout: 100, signal: preAborted.signal }), { name: 'AbortError' });
    assert.equal(calls.length, count);
    assert.equal(timers.size, 0);
  } finally {
    globalThis.setTimeout = set;
    globalThis.clearTimeout = clear;
  }
});
