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
    install('$fetch', ofetch.create({ baseURL: `http://127.0.0.1:${server.address().port}` }));
    install('tryUseNuxtApp', () => undefined);
    install('getCurrentInstance', () => undefined);
    const { $apiFetch } = await loadWrapper(false);
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
