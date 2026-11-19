import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { createRequire } from 'node:module';
import { join } from 'node:path';
import test from 'node:test';
import { rootDirectory as root } from '../generate.mjs';
import { loadWrapper } from './wrapper.mjs';

test('typed GET, PATCH, DELETE and multipart upload reach HTTP with the observed encoded URL', async () => {
  const observed = [];
  const server = createServer(async (request, response) => {
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    observed.push({ method: request.method, url: request.url, headers: request.headers, body: Buffer.concat(chunks).toString() });
    response.setHeader('Content-Type', 'application/json');
    response.end(JSON.stringify({ id: 'sample-object' }));
  });
  await new Promise((accept) => server.listen(0, '127.0.0.1', accept));
  const originals = new Map();
  function install(name, value) {
    originals.set(name, Object.getOwnPropertyDescriptor(globalThis, name));
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value });
  }
  try {
    const require = createRequire(join(root, 'server/package.json'));
    const { ofetch } = require('ofetch');
    const origin = `http://127.0.0.1:${server.address().port}`;
    // Resolve relative requests as a browser would, without custom baseURL defaults.
    install('$fetch', { raw: (url, options) => ofetch.raw(new URL(url, origin).href, options) });
    const { $apiFetch } = await loadWrapper();
    const query = { q: 'Sample & ü', type: 'Game' };
    await $apiFetch('GET /api/v1/admin/search/game', { query });
    const path = { id: 'sample/with %2F?#ü' };
    const body = { mName: 'Sample' };
    await $apiFetch('PATCH /api/v1/admin/game/:id', { path, body });
    await $apiFetch('DELETE /api/v1/admin/company/:id', { path });
    const form = new FormData();
    form.append('image', new Blob(['sample-upload'], { type: 'image/png' }), 'sample.png');
    await $apiFetch('POST /api/v1/admin/company/:id/icon', { path, body: form });

    assert.deepEqual(observed.map(({ method }) => method), ['GET', 'PATCH', 'DELETE', 'POST']);
    const url = new URL(observed[0].url, 'http://example.invalid');
    assert.equal(url.pathname, '/api/v1/admin/search/game');
    assert.deepEqual(Object.fromEntries(url.searchParams), query);
    const encoded = 'sample%2Fwith%20%252F%3F%23%C3%BC';
    assert.equal(observed[1].url, `/api/v1/admin/game/${encoded}`);
    assert.equal(observed[2].url, `/api/v1/admin/company/${encoded}`);
    assert.equal(observed[3].url, `/api/v1/admin/company/${encoded}/icon`);
    assert.deepEqual(JSON.parse(observed[1].body), body);
    assert.equal(observed[2].body, '');
    assert.match(observed[3].headers['content-type'], /^multipart\/form-data; boundary=/);
    assert.match(observed[3].body, /filename="sample.png"/);
    assert.match(observed[3].body, /sample-upload/);
    assert.deepEqual(path, { id: 'sample/with %2F?#ü' });
    assert.deepEqual(body, { mName: 'Sample' });
    assert.equal(form.get('image').name, 'sample.png');
  } finally {
    for (const [name, descriptor] of originals) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else delete globalThis[name];
    }
    server.closeAllConnections();
    await new Promise((accept, reject) => server.close((error) => error ? reject(error) : accept()));
  }
});

test('typed browser responses normalize real ofetch empty bodies and HTTP 204 to null', async () => {
  const server = createServer((request, response) => {
    response.statusCode = request.method === 'GET' ? 204 : 200;
    response.setHeader('Content-Type', 'application/json');
    response.end();
  });
  await new Promise((accept) => server.listen(0, '127.0.0.1', accept));
  const original = Object.getOwnPropertyDescriptor(globalThis, '$fetch');
  try {
    const require = createRequire(join(root, 'server/package.json'));
    const { ofetch } = require('ofetch');
    Object.defineProperty(globalThis, '$fetch', {
      configurable: true, writable: true,
      value: { raw: (url, options) => ofetch.raw(new URL(url, `http://127.0.0.1:${server.address().port}`).href, options) },
    });
    const { $apiFetch } = await loadWrapper();
    assert.equal(await $apiFetch('GET /api/v1/user'), null);
    assert.equal(await $apiFetch('DELETE /api/v1/admin/company/:id', { path: { id: 'sample' } }), null);
    assert.equal(await $apiFetch('HEAD /api/v1/object/:id', { path: { id: 'sample' } }), null);
  } finally {
    if (original) Object.defineProperty(globalThis, '$fetch', original);
    else delete globalThis.$fetch;
    server.closeAllConnections();
    await new Promise((accept, reject) => server.close((error) => error ? reject(error) : accept()));
  }
});

test('failed typed requests show catalog messages only, and a proxy page produces the generic text', async () => {
  const html = '<html><body><script>alert(1)</script>Bad gateway</body></html>';
  const server = createServer((request, response) => {
    if (request.url.startsWith('/api/v1/user')) {
      response.statusCode = 502;
      response.setHeader('Content-Type', 'text/html');
      response.setHeader('X-Request-Id', 'proxy-0001');
      response.end(html);
    } else {
      response.statusCode = 404;
      response.setHeader('Content-Type', 'application/json');
      response.end(JSON.stringify({ code: 'not_found', message: 'The requested resource does not exist.', request_id: 'abc-0002' }));
    }
  });
  await new Promise((accept) => server.listen(0, '127.0.0.1', accept));
  const originals = new Map();
  function install(name, value) {
    originals.set(name, Object.getOwnPropertyDescriptor(globalThis, name));
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value });
  }
  try {
    const require = createRequire(join(root, 'server/package.json'));
    const { ofetch } = require('ofetch');
    const origin = `http://127.0.0.1:${server.address().port}`;
    const modals = [];
    install('$fetch', { raw: (url, options) => ofetch.raw(new URL(url, origin).href, options) });
    install('ModalType', { Notification: 'notification' });
    install('createModal', (type, content) => modals.push({ type, ...content }));
    const { $apiFetch } = await loadWrapper();

    const proxied = await $apiFetch('GET /api/v1/user', { failTitle: 'Failed' }).then(() => null, (error) => error);
    assert.ok(proxied);
    assert.equal(proxied.apiError, undefined);
    assert.ok(proxied instanceof Error);
    assert.equal(proxied.status, 502);
    assert.equal(modals.length, 1);
    assert.equal(modals[0].description, 'The server returned an unexpected response (HTTP 502). Request id: proxy-0001.');
    assert.doesNotMatch(modals[0].description, /html|script|Bad gateway/);

    const missing = await $apiFetch('GET /api/v1/admin/search/game', { query: { q: 'x', type: 'Game' }, failTitle: 'Failed' })
      .then(() => null, (error) => error);
    assert.equal(missing.apiError.code, 'not_found');
    assert.equal(modals.length, 2);
    assert.equal(modals[1].description, 'The requested resource does not exist.');
  } finally {
    for (const [name, descriptor] of originals) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else delete globalThis[name];
    }
    server.closeAllConnections();
    await new Promise((accept, reject) => server.close((error) => error ? reject(error) : accept()));
  }
});
