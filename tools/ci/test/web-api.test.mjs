import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { describe, extraHandlers, handlers, normalize, readInventory, validateInventory, validateRedirectRules } from '../../web-api/inventory.mjs';
import { binaryValues, jsonResponses, opaqueResponses, ordinaryBodies, streamingBodies, unsupportedBody } from '../fixtures/fetch-values.mjs';
import { assertApiFetchOptions, findTransfer, isBinaryRequestBody, isBinaryValue, isTransferResponse, mergeRequestHeaders, requestFingerprint, resolveApiPath, stableRequestKey, transferBodyIdentity } from '../../../server/shared/api/request.ts';

test('route inventory rejects missing, additional and duplicate mappings', () => {
  const file = 'server/server/api/v1/things/[id]/index.get.ts';
  const route = describe(file);
  assert.deepEqual(normalize(file), { file, method: 'GET', path: '/api/v1/things/:id' });
  assert.throws(() => validateInventory([route], []), /Missing handler/);
  assert.throws(() => validateInventory([], [file]), /Unmapped handler/);
  assert.throws(() => validateInventory([route, route], [file]), /Duplicate handler/);
  assert.throws(() => validateInventory([route, { ...route, file: 'another.ts' }], [file, 'another.ts']), /Duplicate method\/path/);
  assert.doesNotThrow(() => validateInventory([route], [file]));
});

test('route normalization preserves methodless routes and separate auth handlers', () => {
  assert.equal(normalize('server/server/api/v1/user/superlevel.ts').method, '*');
  assert.equal(describe('server/server/routes/auth/oidc.get.ts').kind, 'redirect');
  assert.equal(describe('server/server/api/v1/task/index.get.ts').kind, 'websocket');
  assert.equal(describe('server/server/api/v1/object/[id]/index.get.ts').kind, 'binary-or-empty');
  assert.equal(describe('server/server/api/v1/setup.post.ts').kind, 'empty');
  assert.equal(describe('server/server/api/v1/client/user/webtoken.post.ts').kind, 'text');
  assert.throws(() => normalize('server/server/api/v1/[...rest].get.ts'), /Unsupported route/);
});

test('the filesystem scan rejects every unsupported Nitro handler extension in both route trees', async () => {
  const root = await mkdtemp(join(tmpdir(), 'http-inventory-'));
  try {
    // Populate real directories so this exercises discovery, not only validation
    // of a preconstructed list. An unmodified scan must first pass completely.
    await Promise.all([...handlers, ...extraHandlers].map(async (file) => {
      const target = join(root, file);
      await mkdir(dirname(target), { recursive: true });
      await writeFile(target, 'export default () => null;\n');
    }));
    await writeFile(join(root, 'server/nuxt.config.ts'), '{ "/auth/callback/oidc": { redirect: "/api/v1/auth/oidc/callback" } }');
    assert.equal((await readInventory(root)).length, handlers.length + extraHandlers.length);
    for (const directory of ['server/server/api/v1', 'server/server/routes']) {
      for (const extension of ['js', 'mjs', 'cjs', 'mts', 'cts', 'tsx', 'jsx']) {
        const relative = `${directory}/nested/sample.get.${extension}`;
        const target = join(root, relative);
        await mkdir(dirname(target), { recursive: true });
        await writeFile(target, 'export default () => null;\n');
        await assert.rejects(readInventory(root), (error) => {
          assert.match(error.message, /Unsupported Nitro handler extensions/);
          assert.ok(error.message.includes(relative), error.message);
          return true;
        });
        await rm(target);
      }
    }
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('redirect inventory rejects missing, changed and additional rules', () => {
  const rule = '"/auth/callback/oidc": { redirect: "/api/v1/auth/oidc/callback" }';
  assert.doesNotThrow(() => validateRedirectRules(rule));
  assert.throws(() => validateRedirectRules(''), /redirect rule/);
  assert.throws(() => validateRedirectRules(rule.replace('/api/v1/auth/oidc/callback', '/different')), /redirect rule/);
  assert.throws(() => validateRedirectRules(`${rule}, "/extra": { redirect: "/target" }`), /redirect rule/);
  assert.throws(() => validateRedirectRules(`${rule}, "/dynamic": { redirect: target }`), /redirect rule/);
});

test('path parameters are encoded once without mutation or substitution of query data', () => {
  const path = { id: 'a/b %?#ü' };
  assert.equal(resolveApiPath('/api/v1/games/:id', path), '/api/v1/games/a%2Fb%20%25%3F%23%C3%BC');
  assert.deepEqual(path, { id: 'a/b %?#ü' });
  assert.equal(resolveApiPath('/items/:id', { id: '%2F' }), '/items/%252F');
  assert.equal(resolveApiPath('/items/:id', { id: 0 }), '/items/0');
  assert.throws(() => resolveApiPath('/items/:id'), /Missing path/);
  assert.throws(() => resolveApiPath('/items/:id', { id: '' }), /Invalid path/);
  for (const id of ['.', '..']) {
    assert.throws(() => resolveApiPath('/items/:id', { id }), /Invalid path parameter: id/);
  }
  for (const id of ['.hidden', '...', 'game..name', '%2e', '%2E%2E']) {
    const path = resolveApiPath('/items/:id', { id });
    assert.equal(new URL(path, 'https://example.invalid').pathname, path);
  }
  assert.throws(() => resolveApiPath('/items/:id', { id: 'x', query: 'y' }), /Unexpected path/);
});

test('typed requests reject mutable hooks and dispatch overrides before building their identity', () => {
  for (const name of ['onRequest', 'onRequestError', 'onResponse', 'onResponseError']) {
    for (const value of [() => {}, [() => {}]]) {
      assert.throws(() => assertApiFetchOptions({ [name]: value }), /Unsupported typed request option/);
    }
  }
  for (const options of [
    { method: 'POST' }, { responseType: 'text' }, { baseURL: 'https://example.invalid' },
    { parseResponse: () => 'changed' }, { dispatcher: {} }, { agent: {} },
  ]) assert.throws(() => assertApiFetchOptions(options), /Unsupported typed request option/);
  assert.throws(() => assertApiFetchOptions({ retryDelay: () => 0 }), /retryDelay must be a number/);
  assert.doesNotThrow(() => assertApiFetchOptions({ headers: { Authorization: 'Bearer sample' }, retryDelay: 25 }));
});

test('explicit Headers and tuple lists override forwarded credentials by header name', () => {
  const forwarded = { cookie: 'session=sample-a', authorization: 'Bearer sample-a' };
  for (const explicit of [new Headers({ Authorization: 'Bearer sample-b' }), [['Authorization', 'Bearer sample-b']]]) {
    const headers = mergeRequestHeaders(forwarded, explicit);
    assert.equal(headers.get('cookie'), 'session=sample-a');
    assert.equal(headers.get('authorization'), 'Bearer sample-b');
  }
  assert.deepEqual(forwarded, { cookie: 'session=sample-a', authorization: 'Bearer sample-a' });
});

test('form transfer identities preserve fields and file metadata without file contents', () => {
  const form = ({ contents = 'file-bytes-AAAA', name = 'sample.bin', type = 'application/octet-stream', text = 'sample' } = {}) => {
    const body = new FormData();
    body.append('title', text);
    body.append('tag', 'first');
    body.append('tag', 'second');
    body.append('upload', new Blob([contents], { type }), name);
    return body;
  };
  const first = transferBodyIdentity(form());
  assert.deepEqual(first, {
    format: 'form-data',
    entries: [
      { name: 'title', text: 'sample' }, { name: 'tag', text: 'first' }, { name: 'tag', text: 'second' },
      { name: 'upload', file: { name: 'sample.bin', size: 15, type: 'application/octet-stream' } },
    ],
  });
  assert.deepEqual(first, transferBodyIdentity(form({ contents: 'file-bytes-BBBB' })));
  assert.ok(!stableRequestKey(first).includes('file-bytes-AAAA'));
  for (const changed of [{ name: 'other.bin' }, { type: 'text/plain' }, { contents: 'longer-file-bytes' }, { text: 'other' }]) {
    assert.notEqual(stableRequestKey(first), stableRequestKey(transferBodyIdentity(form(changed))));
  }
  const parameters = new URLSearchParams([['tag', 'first'], ['tag', 'second']]);
  assert.deepEqual(transferBodyIdentity(parameters), {
    format: 'url-search-params', entries: [['tag', 'first'], ['tag', 'second']],
  });
  assert.deepEqual(transferBodyIdentity(parameters), transferBodyIdentity(new URLSearchParams(parameters)));
  assert.notDeepEqual(transferBodyIdentity(parameters), transferBodyIdentity(new URLSearchParams([['tag', 'second'], ['tag', 'first']])));
});

test('ofetch empty, JSON, text, toJSON and form-encoded bodies have explicit identities', () => {
  for (const { name, create } of ordinaryBodies) {
    const body = create();
    assert.equal(isBinaryRequestBody(body), false, name);
    assert.notEqual(transferBodyIdentity(body), undefined, name);
    assert.equal(stableRequestKey(transferBodyIdentity(body)), stableRequestKey(transferBodyIdentity(create())), name);
  }
  const date = new Date('2030-01-01T00:00:00.000Z');
  assert.notEqual(stableRequestKey(transferBodyIdentity(date)), stableRequestKey(transferBodyIdentity(date.toJSON())));
  const contentType = 'application/x-www-form-urlencoded';
  assert.deepEqual(transferBodyIdentity({ name: 'sample', missing: undefined }, contentType), {
    format: 'url-search-params', entries: [['name', 'sample'], ['missing', 'undefined']],
  });
  assert.notEqual(stableRequestKey(transferBodyIdentity({ missing: undefined }, contentType)), stableRequestKey(transferBodyIdentity({}, contentType)));
  assert.equal(transferBodyIdentity(unsupportedBody()), undefined);
});

test('all raw binary and streaming body families are recognized without reading them', async () => {
  for (const { name, create } of [...binaryValues, ...streamingBodies]) {
    const body = create();
    assert.equal(isBinaryRequestBody(body), true, name);
    assert.equal(transferBodyIdentity(body), undefined, name);
    if (body instanceof ReadableStream) {
      assert.equal(body.locked, false);
      await body.cancel();
    } else if (typeof body.destroy === 'function') body.destroy();
  }
});

test('only JSON/text/empty results enter the payload, regardless of declared response type', async () => {
  for (const value of jsonResponses) assert.equal(isTransferResponse(value), true);
  for (const { name, create } of binaryValues) {
    const value = create();
    assert.equal(isBinaryValue(value), true, name);
    assert.equal(isTransferResponse(value), false, name);
    if (value instanceof ReadableStream) await value.cancel();
  }
  for (const { name, create } of opaqueResponses) {
    const value = create();
    assert.equal(isTransferResponse(value), false, name);
    if (typeof value?.destroy === 'function') value.destroy();
  }
});

test('transfer entries preserve falsy values and stable request identities', () => {
  for (const value of [false, null, undefined, 0, '']) {
    assert.deepEqual(findTransfer([{ key: 'sample', value }], 'sample'), { key: 'sample', value });
  }
  assert.equal(findTransfer([], 'sample'), undefined);
  assert.equal(stableRequestKey({ b: { y: 1, x: 2 }, a: 0 }), stableRequestKey({ a: 0, b: { x: 2, y: 1 } }));
  assert.notEqual(stableRequestKey({ method: 'GET', query: { id: 'a' } }), stableRequestKey({ method: 'GET', query: { id: 'b' } }));
});

test('request fingerprints match SHA-256 for empty, Unicode and multiple-block inputs', () => {
  for (const value of ['', 'abc', 'sample/ü?token=synthetic', 'a'.repeat(55), 'a'.repeat(56), 'a'.repeat(64), 'a'.repeat(1000)]) {
    assert.equal(requestFingerprint(value), createHash('sha256').update(value).digest('hex'));
  }
});
