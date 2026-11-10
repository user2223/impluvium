import assert from 'node:assert/strict';
import { AsyncLocalStorage } from 'node:async_hooks';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { join } from 'node:path';
import test from 'node:test';
import { pathToFileURL } from 'node:url';
import { compiler } from '../compiler.mjs';
import { rootDirectory as root } from '../generate.mjs';
import { binaryValues, jsonResponses, opaqueResponses, ordinaryBodies, sampleBytes, streamingBodies, unsupportedBody } from '../../ci/fixtures/fetch-values.mjs';

// Exercise the production wrapper with synthetic Nuxt request contexts. No handler
// is imported or started, and there are no HTTP or database connections.
async function loadWrapper(server) {
  await readFile(join(root, 'server/shared/api/generated/routes.ts'), 'utf8');
  const ts = compiler(root);
  const source = await readFile(join(root, 'server/composables/request.ts'), 'utf8');
  let code = ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext },
  }).outputText;
  for (const [alias, file] of [
    ['~/shared/api/generated/routes', 'server/shared/api/generated/routes.ts'],
    ['~/shared/api/request', 'server/shared/api/request.ts'],
  ]) code = code.replaceAll(JSON.stringify(alias), JSON.stringify(pathToFileURL(join(root, file)).href));
  code = code.replaceAll('import.meta.server', String(server)).replaceAll('import.meta.client', String(!server));
  return import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
}

test('both wrappers share request-scoped credentials and transfer data through hydration', async () => {
  const contexts = new AsyncLocalStorage();
  const originals = new Map();
  function install(name, value) {
    originals.set(name, Object.getOwnPropertyDescriptor(globalThis, name));
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value });
  }
  install('tryUseNuxtApp', () => contexts.getStore()?.app);
  install('getCurrentInstance', () => contexts.getStore()?.instance);
  install('useRequestHeaders', () => contexts.getStore().incoming);
  install('useState', (key, initialize) => {
    const state = contexts.getStore().state;
    if (!state.has(key)) state.set(key, { value: initialize() });
    return state.get(key);
  });
  install('$fetch', async (url, options) => {
    const context = contexts.getStore();
    context.calls.push({ url, options });
    if (context.error) throw context.error;
    if (context.fetch) return context.fetch(url, options);
    return context.response;
  });
  const makeContext = (label, incoming = {}) => {
    const hooks = new Map();
    return {
      app: {
        isHydrating: false,
        hook(name, callback) {
          if (!hooks.has(name)) hooks.set(name, []);
          hooks.get(name).push(callback);
        },
      },
      hooks, instance: { proxy: {} }, incoming, state: new Map(), calls: [], response: { label },
    };
  };
  const finishHydration = async (context) => {
    context.app.isHydrating = false;
    for (const callback of context.hooks.get('app:suspense:resolve') ?? []) await callback();
  };
  try {
    const server = await loadWrapper(true);
    const browser = await loadWrapper(false);
    const a = makeContext('sample-a', { cookie: 'session=sample-a' });
    const b = makeContext('sample-b', { authorization: 'Bearer sample-b' });
    // Mixed accesses share a key when their parsing modes also match. Legacy
    // MIME auto-detection is deliberately distinct from explicit JSON parsing.
    await Promise.all([a, b].map((context) => contexts.run(context, async () => {
      const first = await server.$dropFetch('/api/v1', { responseType: 'json' });
      const second = await server.$apiFetch('GET /api/v1');
      assert.deepEqual(second, first);
      assert.equal(context.calls.length, 1);
      for (const [name, value] of Object.entries(context.incoming)) {
        assert.equal(context.calls[0].options.headers.get(name), value);
        assert.ok(!JSON.stringify([...context.state]).includes(value));
      }
    })));
    assert.deepEqual(a.response, { label: 'sample-a' });
    assert.deepEqual(b.response, { label: 'sample-b' });
    assert.notEqual(a.state.get('api-fetch-transfer'), b.state.get('api-fetch-transfer'));

    const concurrentRead = makeContext('concurrent-read');
    let finishRead;
    const readResponse = new Promise((resolve) => { finishRead = resolve; });
    concurrentRead.fetch = () => readResponse;
    await contexts.run(concurrentRead, async () => {
      const first = server.$apiFetch('GET /api/v1');
      const second = server.$dropFetch('/api/v1', { responseType: 'json' });
      finishRead(concurrentRead.response);
      assert.deepEqual(await Promise.all([first, second]), [concurrentRead.response, concurrentRead.response]);
      assert.equal(concurrentRead.calls.length, 1);
      assert.equal(concurrentRead.state.get('api-fetch-transfer').value.length, 1);
    });

    const binaryValue = (type) => {
      const bytes = new TextEncoder().encode('sample-binary-response');
      if (type === 'blob') return new Blob([bytes], { type: 'application/octet-stream' });
      if (type === 'arrayBuffer') return bytes.buffer;
      return new ReadableStream({ start(controller) { controller.enqueue(bytes); controller.close(); } });
    };
    const binaryText = (type, value) => {
      if (type === 'blob') {
        assert.ok(value instanceof Blob);
        return value.text();
      }
      if (type === 'arrayBuffer') {
        assert.ok(value instanceof ArrayBuffer);
        return new TextDecoder().decode(value);
      }
      assert.ok(value instanceof ReadableStream);
      return new Response(value).text();
    };
    const readBinary = (wrapper, type, typed = false) => typed && type === 'blob'
      ? wrapper.$apiFetch('GET /api/v1/object/:id', { path: { id: 'sample' } })
      : wrapper.$dropFetch('/api/v1/object/sample', { responseType: type });
    for (const type of ['blob', 'arrayBuffer', 'stream']) {
      const binary = makeContext(`binary-${type}`, { cookie: `session=sample-${type}` });
      let complete;
      const response = new Promise((resolve) => { complete = resolve; });
      binary.fetch = () => response;
      await contexts.run(binary, async () => {
        const pending = [readBinary(server, type, true), readBinary(server, type)];
        if (type === 'stream') pending.push(readBinary(server, type));
        assert.equal(binary.calls.length, 1);
        complete(binaryValue(type));
        const values = await Promise.all(pending);
        assert.notEqual(values[0], values[1]);
        assert.deepEqual(await Promise.all(values.map((value) => binaryText(type, value))), values.map(() => 'sample-binary-response'));
        if (type === 'arrayBuffer') {
          new Uint8Array(values[0])[0] = 0;
          assert.notEqual(new Uint8Array(values[0])[0], new Uint8Array(values[1])[0]);
        }
        assert.equal(binary.state.get('api-fetch-transfer').value.length, 0);
        // Later readers get new copies of the same request-scoped snapshot,
        // including streams whose earlier branches have already been consumed.
        binary.fetch = () => { throw new Error('Unexpected repeated binary fetch'); };
        assert.equal(await binaryText(type, await readBinary(server, type, true)), 'sample-binary-response');
        assert.equal(await binaryText(type, await readBinary(server, type)), 'sample-binary-response');
        assert.equal(binary.calls.length, 1);
        assert.equal(binary.state.get('api-fetch-transfer').value.length, 0);
      });
      const separateRequest = makeContext(`binary-${type}-separate-request`);
      separateRequest.fetch = () => binaryValue(type);
      await contexts.run(separateRequest, async () => {
        assert.equal(await binaryText(type, await readBinary(server, type, true)), 'sample-binary-response');
        assert.equal(separateRequest.calls.length, 1);
      });
      const binaryClient = makeContext(`binary-${type}-browser`);
      binaryClient.state = structuredClone(binary.state);
      binaryClient.app.isHydrating = true;
      binaryClient.fetch = () => binaryValue(type);
      await contexts.run(binaryClient, async () => {
        for (const typed of [true, false]) {
          assert.equal(await binaryText(type, await readBinary(browser, type, typed)), 'sample-binary-response');
        }
        assert.equal(binaryClient.calls.length, 2);
        assert.equal(binaryClient.state.get('api-fetch-transfer').value.length, 0);
        await finishHydration(binaryClient);
      });

      for (const [wrapper, hydrating, setup] of [
        [server, false, true], [server, false, false],
        [browser, true, true], [browser, true, false], [browser, false, true],
      ]) {
        const unsafeMutation = makeContext(`binary-mutation-${type}`);
        unsafeMutation.app.isHydrating = hydrating;
        if (!setup) unsafeMutation.instance = undefined;
        await contexts.run(unsafeMutation, async () => {
          await assert.rejects(wrapper.$dropFetch('/api/v1/collection', {
            method: 'POST', body: { name: 'Sample' }, responseType: type,
          }), /Binary mutation responses cannot be transferred through hydration/);
          assert.equal(unsafeMutation.calls.length, 0);
          assert.equal(unsafeMutation.state.get('api-fetch-transfer').value.length, 0);
        });
      }
      const action = makeContext(`binary-action-${type}`);
      action.instance = undefined;
      action.fetch = () => binaryValue(type);
      await contexts.run(action, async () => {
        const value = await browser.$dropFetch('/api/v1/collection', {
          method: 'POST', body: { name: 'Sample' }, responseType: type,
        });
        assert.equal(await binaryText(type, value), 'sample-binary-response');
        assert.equal(action.calls.length, 1);
      });
    }

    // Exercise every binary runtime family with no responseType at all, plus a
    // parser-style mismatch. Actual results, not declared options, choose copies.
    const valueBytes = async (value) => {
      if (value instanceof Blob) return [...new Uint8Array(await value.arrayBuffer())];
      if (ArrayBuffer.isView(value)) return [...new Uint8Array(value.buffer, value.byteOffset, value.byteLength)];
      if (value instanceof ReadableStream) return [...new Uint8Array(await new Response(value).arrayBuffer())];
      return [...new Uint8Array(value)];
    };
    for (const { name, create } of binaryValues) {
      const actual = create();
      const expected = actual instanceof ReadableStream ? [...sampleBytes()] : await valueBytes(actual);
      const context = makeContext(`automatic-${name}`);
      let finish;
      const result = new Promise((resolve) => { finish = resolve; });
      context.fetch = () => result;
      await contexts.run(context, async () => {
        const first = server.$dropFetch('/api/v1/object/sample');
        const second = server.$dropFetch('/api/v1/object/sample');
        finish(actual);
        const values = await Promise.all([first, second]);
        assert.equal(context.calls.length, 1, name);
        assert.notEqual(values[0], values[1], name);
        for (const value of values) assert.equal(value.constructor, actual.constructor, name);
        assert.deepEqual(await Promise.all(values.map(valueBytes)), [expected, expected], name);
        if (actual instanceof File) {
          assert.equal(values[0].name, actual.name);
          assert.equal(values[0].lastModified, actual.lastModified);
          assert.equal(values[0].type, actual.type);
        }
        if (ArrayBuffer.isView(values[0])) {
          new Uint8Array(values[0].buffer, values[0].byteOffset, values[0].byteLength)[0] = 255;
          assert.deepEqual(await valueBytes(values[1]), expected, name);
        }
        assert.equal(context.state.get('api-fetch-transfer').value.length, 0, name);
        const later = await server.$dropFetch('/api/v1/object/sample');
        assert.equal(later.constructor, actual.constructor, name);
        assert.notEqual(later, values[0], name);
        assert.deepEqual(await valueBytes(later), expected, name);
        assert.equal(context.calls.length, 1, name);
        context.fetch = create;
        // Even an explicit JSON hint cannot turn a parser-returned binary into {}.
        const hinted = await server.$dropFetch('/api/v1/object/sample', { responseType: 'json' });
        assert.equal(hinted.constructor, actual.constructor, name);
        assert.deepEqual(await valueBytes(hinted), expected, name);
        assert.equal(context.calls.length, 2, name);
        assert.equal(context.state.get('api-fetch-transfer').value.length, 0, name);
      });
      const client = makeContext(`automatic-${name}-browser`);
      client.state = structuredClone(context.state);
      client.app.isHydrating = true;
      client.fetch = create;
      await contexts.run(client, async () => {
        for (let index = 0; index < 2; index++) {
          const value = await browser.$dropFetch('/api/v1/object/sample');
          assert.equal(value.constructor, actual.constructor, name);
          assert.deepEqual(await valueBytes(value), expected, name);
        }
        assert.equal(client.calls.length, 2, name);
        await finishHydration(client);
      });
    }

    const rejectRenderBody = async ({ name, create }, message) => {
      const body = create();
      try {
        for (const [wrapper, hydrating, setup, withoutApp] of [
          [server, false, true], [server, false, false],
          [server, false, false, true],
          [browser, true, true], [browser, true, false], [browser, false, true],
        ]) {
          const context = makeContext(`render-body-${name}`);
          context.app.isHydrating = hydrating;
          if (!setup) context.instance = undefined;
          if (withoutApp) context.app = undefined;
          await contexts.run(context, async () => {
            await assert.rejects(wrapper.$dropFetch('/api/v1/screenshots/game/sample', {
              method: 'POST', body, duplex: 'half',
            }), message, name);
            if (body instanceof Blob || body instanceof ArrayBuffer || body instanceof Uint8Array) {
              await assert.rejects(wrapper.$apiFetch('POST /api/v1/screenshots/game/:id', {
                path: { id: 'sample' }, body,
              }), message, `${name}, typed access`);
            }
            assert.equal(context.calls.length, 0, name);
            assert.equal(context.state.get('api-fetch-transfer')?.value.length ?? 0, 0, name);
          });
        }
        const event = makeContext(`body-event-${name}`);
        event.instance = undefined;
        await contexts.run(event, async () => {
          assert.strictEqual(await browser.$dropFetch('/api/v1/screenshots/game/sample', {
            method: 'POST', body, duplex: 'half',
          }), event.response, name);
          assert.strictEqual(event.calls[0].options.body, body, name);
          assert.equal(event.calls.length, 1, name);
        });
      } finally {
        if (body instanceof ReadableStream) {
          assert.equal(body.locked, false, name);
          await body.cancel();
        } else if (typeof body.destroy === 'function') body.destroy();
      }
    };
    for (const fixture of [...binaryValues, ...streamingBodies]) {
      await rejectRenderBody(fixture, /Binary mutation bodies cannot be transferred through hydration/);
    }
    await rejectRenderBody({ name: 'unsupported', create: unsupportedBody }, /body cannot be identified for hydration/);

    // Normal ofetch bodies, including toJSON and form-encoded JSON objects,
    // retain mutation FIFO transfer and are forwarded without modification.
    for (const { name, create, headers } of [
      ...ordinaryBodies,
      { name: 'form-encoded record', create: () => ({ name: 'sample', missing: undefined }), headers: { 'Content-Type': 'application/x-www-form-urlencoded' } },
    ]) {
      const body = create();
      const context = makeContext(`ordinary-body-${name}`);
      context.fetch = () => ({ id: context.calls.length });
      const invoke = (wrapper) => wrapper.$dropFetch('/api/v1/collection', { method: 'POST', body, headers });
      await contexts.run(context, async () => {
        assert.deepEqual(await invoke(server), { id: 1 }, name);
        assert.deepEqual(await invoke(server), { id: 2 }, name);
        assert.equal(context.calls.length, 2, name);
        assert.strictEqual(context.calls[0].options.body, body, name);
      });
      const client = makeContext(`ordinary-body-${name}-browser`);
      client.state = structuredClone(context.state);
      client.app.isHydrating = true;
      await contexts.run(client, async () => {
        assert.deepEqual(await invoke(browser), { id: 1 }, name);
        assert.deepEqual(await invoke(browser), { id: 2 }, name);
        assert.equal(client.calls.length, 0, name);
        await finishHydration(client);
      });
    }

    for (const value of jsonResponses) {
      const context = makeContext('json-response');
      context.response = value;
      await contexts.run(context, async () => {
        assert.strictEqual(await server.$dropFetch('/api/v1', { responseType: 'json' }), value);
        assert.deepEqual(await server.$apiFetch('GET /api/v1'), value);
        assert.equal(context.calls.length, 1);
      });
      const client = makeContext('json-response-browser');
      client.state = structuredClone(context.state);
      client.app.isHydrating = true;
      await contexts.run(client, async () => {
        assert.deepEqual(await browser.$dropFetch('/api/v1', { responseType: 'json' }), value);
        assert.deepEqual(await browser.$apiFetch('GET /api/v1'), value);
        assert.equal(client.calls.length, 0);
        await finishHydration(client);
      });
    }

    for (const { name, create } of opaqueResponses) {
      const context = makeContext(`opaque-${name}`);
      const produced = [];
      context.fetch = () => { const value = create(); produced.push(value); return value; };
      await contexts.run(context, async () => {
        const first = server.$dropFetch('/api/v1/custom-parser');
        const second = server.$dropFetch('/api/v1/custom-parser');
        const values = await Promise.all([first, second]);
        assert.equal(context.calls.length, 2, name);
        assert.strictEqual(values[0], produced[0], name);
        assert.strictEqual(values[1], produced[1], name);
        assert.equal(context.state.get('api-fetch-transfer').value.length, 0, name);
      });
      for (const value of produced) if (typeof value?.destroy === 'function') value.destroy();
    }

    // A binary/opaque result discovered after a mutation ran cannot safely be
    // recovered by repeating that side effect during hydration, even on retry.
    for (const { name, create, blocked } of [
      ...binaryValues.map((fixture) => ({ ...fixture, blocked: 'binary' })),
      ...opaqueResponses.map((fixture) => ({ ...fixture, blocked: 'opaque' })),
    ]) {
      const actual = create();
      const context = makeContext(`late-mutation-${name}`);
      context.response = actual;
      await contexts.run(context, async () => {
        assert.strictEqual(await server.$dropFetch('/api/v1/collection', { method: 'POST', body: { name: 'A' } }), actual, name);
        assert.equal(context.calls.length, 1, name);
        const [entry] = context.state.get('api-fetch-transfer').value;
        assert.equal(entry.blocked, blocked, name);
        assert.equal(entry.value, undefined, name);
        assert.doesNotThrow(() => JSON.stringify([...context.state]), name);
      });
      const client = makeContext(`late-mutation-${name}-browser`);
      client.state = structuredClone(context.state);
      client.app.isHydrating = true;
      await contexts.run(client, async () => {
        for (let index = 0; index < 2; index++) {
          await assert.rejects(browser.$dropFetch('/api/v1/collection', { method: 'POST', body: { name: 'A' } }), /Mutation response cannot be replayed during hydration/, name);
        }
        assert.equal(client.calls.length, 0, name);
        await finishHydration(client);
        client.instance = undefined;
        client.response = actual;
        assert.strictEqual(await browser.$dropFetch('/api/v1/collection', { method: 'POST', body: { name: 'A' } }), actual, name);
        assert.equal(client.calls.length, 1, name);
      });
      // The opaque proxy fixture throws on prototype inspection; only real streams need cleanup.
      let stream = false;
      try { stream = actual instanceof ReadableStream; } catch { stream = false; }
      if (stream) await actual.cancel();
      else if (typeof actual?.destroy === 'function') actual.destroy();
    }

    // Run real ofetch parsing against synthetic native Responses, without HTTP.
    // This covers MIME auto-detection as well as custom legacy parser results.
    const { createFetch } = createRequire(join(root, 'server/package.json'))('ofetch');
    for (const fixture of [
      { name: 'JSON MIME', payload: '{"name":"sample"}', contentType: 'application/json', expected: { name: 'sample' }, transfer: true },
      { name: 'text MIME', payload: 'sample', contentType: 'text/plain', expected: 'sample', transfer: true },
      { name: 'empty HTTP response', payload: null, status: 204, expected: undefined, transfer: true },
      { name: 'binary MIME', payload: sampleBytes(), contentType: 'application/octet-stream', binary: true, bytes: [...sampleBytes()] },
      { name: 'event-stream MIME', payload: 'data: sample\n\n', contentType: 'text/event-stream', binary: true, bytes: [...new TextEncoder().encode('data: sample\n\n')] },
      { name: 'binary parser', payload: '{}', contentType: 'application/json', options: { parseResponse: () => sampleBytes() }, binary: true, bytes: [...sampleBytes()] },
      { name: 'opaque parser', payload: '{}', contentType: 'application/json', options: { parseResponse: () => new Date('2030-01-01T00:00:00.000Z') } },
    ]) {
      let nativeCalls = 0;
      const ofetch = createFetch({
        Headers, AbortController,
        fetch: async () => {
          nativeCalls++;
          return new Response(fixture.payload, {
            status: fixture.status ?? 200,
            headers: fixture.contentType ? { 'Content-Type': fixture.contentType } : undefined,
          });
        },
      });
      const context = makeContext(fixture.name);
      context.fetch = ofetch;
      const verify = async (values) => {
        if (fixture.binary) assert.deepEqual(await Promise.all(values.map(valueBytes)), [fixture.bytes, fixture.bytes], fixture.name);
        else if (fixture.transfer) assert.deepEqual(values, [fixture.expected, fixture.expected], fixture.name);
        else for (const value of values) assert.ok(value instanceof Date, fixture.name);
      };
      await contexts.run(context, async () => {
        const first = server.$dropFetch('/api/v1/ofetch-shapes', fixture.options);
        const second = server.$dropFetch('/api/v1/ofetch-shapes', fixture.options);
        await verify(await Promise.all([first, second]));
        assert.equal(context.calls.length, fixture.options ? 2 : 1, fixture.name);
        assert.equal(context.state.get('api-fetch-transfer').value.length, fixture.transfer ? 1 : 0, fixture.name);
      });
      const serverCalls = nativeCalls;
      const client = makeContext(`${fixture.name}-browser`);
      client.state = structuredClone(context.state);
      client.app.isHydrating = true;
      client.fetch = ofetch;
      await contexts.run(client, async () => {
        await verify(await Promise.all([
          browser.$dropFetch('/api/v1/ofetch-shapes', fixture.options),
          browser.$dropFetch('/api/v1/ofetch-shapes', fixture.options),
        ]));
        assert.equal(client.calls.length, fixture.transfer ? 0 : 2, fixture.name);
        assert.equal(nativeCalls - serverCalls, fixture.transfer ? 0 : 2, fixture.name);
        await finishHydration(client);
      });
    }

    // Default MIME detection and explicit JSON must never share a value, in
    // either invocation order, for sequential or concurrent server consumers.
    for (const contentType of ['text/plain', 'application/octet-stream']) {
      for (const modes of [[undefined, 'json'], ['json', undefined]]) {
        for (const concurrent of [false, true]) {
          let nativeCalls = 0;
          const ofetch = createFetch({
            Headers, AbortController,
            fetch: async () => {
              nativeCalls++;
              return new Response('true', { headers: { 'Content-Type': contentType } });
            },
          });
          const context = makeContext('distinct-response-modes');
          context.instance = undefined;
          context.fetch = ofetch;
          const read = (wrapper, mode) => wrapper.$dropFetch('/api/v1/object/sample', mode ? { responseType: mode } : undefined);
          const verify = async (mode, value) => {
            if (mode === 'json') assert.strictEqual(value, true);
            else if (contentType === 'text/plain') assert.strictEqual(value, 'true');
            else {
              assert.ok(value instanceof Blob);
              assert.equal(await value.text(), 'true');
            }
          };
          await contexts.run(context, async () => {
            if (concurrent) {
              const values = await Promise.all(modes.map((mode) => read(server, mode)));
              for (const [index, mode] of modes.entries()) await verify(mode, values[index]);
            } else {
              for (const mode of modes) await verify(mode, await read(server, mode));
            }
            for (const mode of modes) await verify(mode, await read(server, mode));
            assert.equal(nativeCalls, 2);
            assert.equal(context.calls.length, 2);
            const entries = context.state.get('api-fetch-transfer').value;
            assert.equal(entries.length, contentType === 'text/plain' ? 2 : 1);
            if (entries.length === 2) assert.notEqual(entries[0].key, entries[1].key);
          });
          const client = makeContext('distinct-response-modes-browser');
          client.state = structuredClone(context.state);
          client.app.isHydrating = true;
          client.fetch = ofetch;
          await contexts.run(client, async () => {
            for (const mode of [...modes, ...modes]) await verify(mode, await read(browser, mode));
            assert.equal(client.calls.length, contentType === 'text/plain' ? 0 : 2);
            await finishHydration(client);
          });
        }
      }
    }

    // Custom behavior must not reuse values produced under other parsing or
    // credential callbacks, even if the URL and declared response type match.
    for (const options of [
      { parseResponse: () => sampleBytes() }, { onRequest: () => {} },
      { onRequestError: () => {} }, { onResponse: () => {} }, { onResponseError: () => {} },
      { retryDelay: () => 0 }, { agent: {} }, { dispatcher: {} },
    ]) {
      const context = makeContext('custom-fetch-behavior');
      context.fetch = () => ({ invocation: context.calls.length });
      await contexts.run(context, async () => {
        assert.deepEqual(await server.$dropFetch('/api/v1'), { invocation: 1 });
        assert.deepEqual(await server.$dropFetch('/api/v1', options), { invocation: 2 });
        assert.deepEqual(await server.$dropFetch('/api/v1', options), { invocation: 3 });
        assert.equal(context.state.get('api-fetch-transfer').value.length, 1);
        await assert.rejects(server.$dropFetch('/api/v1/collection', {
          ...options, method: 'POST', body: { name: 'A' },
        }), /Custom fetch callbacks or dispatchers cannot be replayed during hydration/);
        assert.equal(context.calls.length, 3);
      });
      const event = makeContext('custom-fetch-behavior-event');
      event.instance = undefined;
      await contexts.run(event, async () => {
        assert.strictEqual(await browser.$dropFetch('/api/v1/collection', {
          ...options, method: 'POST', body: { name: 'A' },
        }), event.response);
        assert.equal(event.calls.length, 1);
      });
    }

    for (const [url, key, token] of [
      ['/api/v1/user', 'GET /api/v1/user', 'sample-user-token'],
      ['/api/v1/admin/users', 'GET /api/v1/admin/users', 'sample-system-token'],
    ]) {
      const context = makeContext(token, { authorization: `Bearer ${token}` });
      await contexts.run(context, async () => {
        await server.$dropFetch(url, { responseType: 'json' });
        await server.$apiFetch(key);
        assert.equal(context.calls.length, 1);
        assert.equal(context.calls[0].options.headers.get('authorization'), `Bearer ${token}`);
        assert.equal(context.calls[0].options.headers.has('cookie'), false);
      });
    }

    // An explicit incoming credential has the same normalized identity in SSR
    // and hydration; a different explicit credential must still remain separate.
    for (const [header, value, override] of [
      ['Authorization', 'Bearer sample-incoming', 'Bearer sample-other'],
      ['Cookie', 'session=sample-incoming', 'session=sample-other'],
    ]) {
      for (const [ssrTyped, clientTyped] of [[false, false], [true, true], [false, true], [true, false]]) {
        const context = makeContext('explicit-credentials', { [header.toLowerCase()]: value });
        context.fetch = () => ({ id: context.calls.length });
        const invoke = (wrapper, typed, credential) => {
          const options = { body: { name: 'A' }, headers: credential === undefined ? undefined : { [header]: credential } };
          return typed ? wrapper.$apiFetch('POST /api/v1/collection', options)
            : wrapper.$dropFetch('/api/v1/collection', { ...options, method: 'POST', responseType: 'json' });
        };
        await contexts.run(context, async () => {
          assert.deepEqual(await invoke(server, ssrTyped, value), { id: 1 });
          assert.deepEqual(await invoke(server, ssrTyped, undefined), { id: 2 });
          assert.deepEqual(await invoke(server, ssrTyped, override), { id: 3 });
          assert.equal(context.calls.length, 3);
          for (const [index, credential] of [value, value, override].entries()) {
            assert.equal(context.calls[index].options.headers.get(header), credential);
          }
          const entries = context.state.get('api-fetch-transfer').value;
          assert.equal(entries[0].key, entries[1].key);
          assert.notEqual(entries[0].key, entries[2].key);
          for (const entry of entries) assert.match(entry.key, /^[a-f0-9]{64}$/);
          assert.ok(!JSON.stringify([...context.state]).includes(value));
          assert.ok(!JSON.stringify([...context.state]).includes(override));
        });
        const client = makeContext('explicit-credentials-browser');
        client.state = structuredClone(context.state);
        client.app.isHydrating = true;
        await contexts.run(client, async () => {
          assert.deepEqual(await invoke(browser, clientTyped, override), { id: 3 });
          assert.deepEqual(await invoke(browser, clientTyped, value), { id: 1 });
          assert.deepEqual(await invoke(browser, clientTyped, undefined), { id: 2 });
          assert.equal(client.calls.length, 0);
          assert.equal(client.state.get('api-fetch-transfer').value.length, 0);
          await finishHydration(client);
          assert.deepEqual(client.state.get('api-fetch-credentials').value, {});
        });
      }
    }

    await contexts.run(a, async () => {
      await server.$apiFetch('GET /api/v1', { headers: new Headers({ Authorization: 'Bearer sample-override' }) });
      assert.equal(a.calls.length, 2);
      assert.equal(a.calls[1].options.headers.get('authorization'), 'Bearer sample-override');
      assert.equal(a.calls[1].options.headers.get('cookie'), 'session=sample-a');
      assert.ok(!JSON.stringify([...a.state]).includes('Bearer sample-override'));
      // The existing user composable repeats the incoming headers explicitly.
      await server.$dropFetch('/api/v1/user', { headers: a.incoming });
    });
    const hydrated = makeContext('browser');
    hydrated.state = structuredClone(a.state);
    hydrated.app.isHydrating = true;
    await contexts.run(hydrated, async () => {
      assert.deepEqual(await browser.$apiFetch('GET /api/v1'), a.response);
      // Layout and page consumers can read the same GET through either access.
      const separateCopy = await browser.$dropFetch('/api/v1', { responseType: 'json' });
      separateCopy.label = 'changed-by-one-consumer';
      hydrated.instance = undefined;
      assert.deepEqual(await browser.$apiFetch('GET /api/v1'), a.response);
      assert.deepEqual(await browser.$dropFetch('/api/v1/user'), a.response);
      assert.equal(hydrated.calls.length, 0);
      assert.ok(hydrated.state.get('api-fetch-transfer').value.length > 0);
      assert.equal(hydrated.hooks.get('app:suspense:resolve').length, 1);
      await finishHydration(hydrated);
      // Cleanup happens at hydration completion, without waiting for a fetch.
      assert.equal(hydrated.state.get('api-fetch-transfer').value.length, 0);
      assert.deepEqual(hydrated.state.get('api-fetch-credentials').value, {});
      assert.deepEqual(await browser.$apiFetch('GET /api/v1'), hydrated.response);
      assert.equal(hydrated.calls.length, 1);
    });

    const head = makeContext('head');
    head.response = undefined;
    await contexts.run(head, async () => {
      assert.equal(await server.$apiFetch('HEAD /api/v1/object/:id', { path: { id: 'sample' } }), undefined);
      assert.equal(await server.$dropFetch('/api/v1/object/sample', { method: 'HEAD', responseType: 'json' }), undefined);
      assert.equal(head.calls.length, 1);
    });
    const headClient = makeContext('head-browser');
    headClient.state = structuredClone(head.state);
    headClient.app.isHydrating = true;
    await contexts.run(headClient, async () => {
      for (let index = 0; index < 2; index++) {
        assert.equal(await browser.$apiFetch('HEAD /api/v1/object/:id', { path: { id: 'sample' } }), undefined);
        assert.equal(await browser.$dropFetch('/api/v1/object/sample', { method: 'HEAD', responseType: 'json' }), undefined);
      }
      assert.equal(headClient.calls.length, 0);
      await finishHydration(headClient);
      assert.equal(headClient.state.get('api-fetch-transfer').value.length, 0);
    });

    // The TOTP setup page issues this POST during setup. A hydration fetch would
    // replace its server-generated secret, so both interfaces must replay the
    // transferred response exactly once, including when setup mixes interfaces.
    for (const [ssrAccess, browserAccess] of [
      ['$dropFetch', '$dropFetch'], ['$apiFetch', '$apiFetch'],
      ['$dropFetch', '$apiFetch'], ['$apiFetch', '$dropFetch'],
    ]) {
      const start = (wrapper, access) => access === '$dropFetch'
        ? wrapper.$dropFetch('/api/v1/user/mfa/totp/start', { method: 'POST', responseType: 'json' })
        : wrapper.$apiFetch('POST /api/v1/user/mfa/totp/start');
      const setup = makeContext('setup', { cookie: 'session=sample-totp' });
      setup.response = { secret: 'sample-original-secret', url: 'otpauth://sample-original' };
      await contexts.run(setup, async () => {
        assert.deepEqual(await start(server, ssrAccess), setup.response);
        assert.equal(setup.calls.length, 1);
        assert.equal(setup.calls[0].options.method, 'POST');
        assert.equal(setup.state.get('api-fetch-transfer').value.length, 1);
      });
      const client = makeContext('setup-browser');
      client.response = { secret: 'sample-replacement-secret', url: 'otpauth://sample-replacement' };
      client.state = structuredClone(setup.state);
      client.app.isHydrating = true;
      await contexts.run(client, async () => {
        assert.deepEqual(await start(browser, browserAccess), setup.response);
        assert.equal(client.calls.length, 0);
        assert.equal(client.state.get('api-fetch-transfer').value.length, 0);
        // A later user action is a fresh mutation, outside component setup.
        await finishHydration(client);
        client.instance = undefined;
        assert.deepEqual(await start(browser, browserAccess), client.response);
        assert.equal(client.calls.length, 1);
      });
    }

    const posted = makeContext('posted');
    const names = ['Sample A', 'Sample B'];
    await contexts.run(posted, async () => {
      for (const name of names) {
        posted.response = { name };
        assert.deepEqual(await server.$apiFetch('POST /api/v1/collection', { body: { name } }), { name });
      }
      assert.equal(posted.calls.length, 2);
      const entries = posted.state.get('api-fetch-transfer').value;
      assert.equal(entries.length, 2);
      assert.notEqual(entries[0].key, entries[1].key);
    });
    const postedClient = makeContext('posted-browser');
    postedClient.state = structuredClone(posted.state);
    postedClient.app.isHydrating = true;
    await contexts.run(postedClient, async () => {
      for (const name of [...names].reverse()) {
        assert.deepEqual(await browser.$dropFetch('/api/v1/collection', { method: 'POST', body: { name }, responseType: 'json' }), { name });
      }
      assert.equal(postedClient.calls.length, 0);
    });

    const formBody = (format, contents) => {
      if (format === 'url-search-params') {
        return new URLSearchParams([['logout_token', 'sample'], ['tag', 'first'], ['tag', 'second']]);
      }
      const body = new FormData();
      body.append('title', 'Sample');
      body.append('upload', new Blob([contents], { type: 'application/octet-stream' }), 'sample.bin');
      return body;
    };
    for (const format of ['form-data', 'url-search-params']) {
      for (const responseKind of ['json', 'text', 'empty']) {
        const url = format === 'form-data' ? '/api/v1/admin/company/sample/banner' : '/api/v1/auth/oidc/logout';
        const upload = (wrapper, typed, contents) => {
          const body = formBody(format, contents);
          if (typed && responseKind !== 'text') {
            return format === 'form-data'
              ? wrapper.$apiFetch('POST /api/v1/admin/company/:id/banner', { path: { id: 'sample' }, body })
              : wrapper.$apiFetch('POST /api/v1/auth/oidc/logout', { body });
          }
          return wrapper.$dropFetch(url, { method: 'POST', body, responseType: responseKind === 'text' ? 'text' : 'json' });
        };
        const expected = responseKind === 'json' ? [{ id: 'first-upload' }, { id: 'second-upload' }]
          : responseKind === 'text' ? ['first-upload', 'second-upload'] : [undefined, undefined];
        const forms = makeContext(`${format}-${responseKind}`, { cookie: 'session=sample-upload' });
        forms.fetch = () => expected[forms.calls.length - 1];
        await contexts.run(forms, async () => {
          assert.deepEqual(await upload(server, true, 'file-bytes-AAAA'), expected[0]);
          assert.deepEqual(await upload(server, false, 'file-bytes-BBBB'), expected[1]);
          assert.equal(forms.calls.length, 2);
          const entries = forms.state.get('api-fetch-transfer').value;
          assert.equal(entries.length, 2);
          assert.equal(entries[0].key, entries[1].key);
          assert.deepEqual(entries.map((entry) => entry.value), expected);
          assert.ok(!JSON.stringify([...forms.state]).includes('file-bytes-'));
          assert.ok(forms.calls[0].options.body instanceof (format === 'form-data' ? FormData : URLSearchParams));
        });
        const client = makeContext(`${format}-${responseKind}-browser`);
        client.state = structuredClone(forms.state);
        client.app.isHydrating = true;
        client.response = responseKind === 'json' ? { id: 'fresh-upload' } : responseKind === 'text' ? 'fresh-upload' : undefined;
        await contexts.run(client, async () => {
          assert.deepEqual(await upload(browser, false, 'file-bytes-CCCC'), expected[0]);
          assert.deepEqual(await upload(browser, true, 'file-bytes-DDDD'), expected[1]);
          assert.equal(client.calls.length, 0);
          assert.equal(client.state.get('api-fetch-transfer').value.length, 0);
          assert.deepEqual(await upload(browser, true, 'file-bytes-EEEE'), client.response);
          assert.equal(client.calls.length, 1);
          await finishHydration(client);
        });
      }
    }

    const createCollection = (wrapper, access) => access === '$dropFetch'
      ? wrapper.$dropFetch('/api/v1/collection', { method: 'POST', body: { name: 'A' }, responseType: 'json' })
      : wrapper.$apiFetch('POST /api/v1/collection', { body: { name: 'A' } });
    let queueNumber = 0;
    for (const [firstAccess, secondAccess] of [
      ['$dropFetch', '$dropFetch'], ['$apiFetch', '$apiFetch'],
      ['$dropFetch', '$apiFetch'], ['$apiFetch', '$dropFetch'],
    ]) {
      const label = `queue-${++queueNumber}`;
      const queued = makeContext(label, { cookie: `session=${label}` });
      queued.fetch = () => ({ id: `${label}-${queued.calls.length}` });
      const expected = [{ id: `${label}-1` }, { id: `${label}-2` }];
      await contexts.run(queued, async () => {
        assert.deepEqual(await createCollection(server, firstAccess), expected[0]);
        assert.deepEqual(await createCollection(server, secondAccess), expected[1]);
        assert.equal(queued.calls.length, 2);
        assert.equal(queued.calls[0].options.headers.get('cookie'), `session=${label}`);
        const entries = queued.state.get('api-fetch-transfer').value;
        assert.equal(entries.length, 2);
        assert.equal(entries[0].key, entries[1].key);
        assert.deepEqual(entries.map((entry) => entry.value), expected);
      });
      const client = makeContext(`${label}-browser`);
      client.state = structuredClone(queued.state);
      client.app.isHydrating = true;
      await contexts.run(client, async () => {
        // Responses are ordered by invocation, independently of which access is used.
        assert.deepEqual(await createCollection(browser, secondAccess), expected[0]);
        assert.deepEqual(await createCollection(browser, firstAccess), expected[1]);
        assert.equal(client.calls.length, 0);
        assert.equal(client.state.get('api-fetch-transfer').value.length, 0);
        assert.deepEqual(await createCollection(browser, firstAccess), client.response);
        assert.equal(client.calls.length, 1);
        await finishHydration(client);
      });
      // Consuming a browser's queue never changes the originating server state.
      assert.deepEqual(queued.state.get('api-fetch-transfer').value.map((entry) => entry.value), expected);
    }

    const overlapping = makeContext('overlapping');
    const completions = [];
    overlapping.fetch = () => new Promise((resolve) => { completions.push(resolve); });
    await contexts.run(overlapping, async () => {
      const first = createCollection(server, '$apiFetch');
      const second = createCollection(server, '$dropFetch');
      assert.equal(overlapping.calls.length, 2);
      completions[1]({ id: 'second' });
      assert.deepEqual(await second, { id: 'second' });
      completions[0]({ id: 'first' });
      assert.deepEqual(await first, { id: 'first' });
      assert.deepEqual(overlapping.state.get('api-fetch-transfer').value.map((entry) => entry.value), [{ id: 'first' }, { id: 'second' }]);
    });
    const overlappingClient = makeContext('overlapping-browser');
    overlappingClient.state = structuredClone(overlapping.state);
    overlappingClient.app.isHydrating = true;
    await contexts.run(overlappingClient, async () => {
      assert.deepEqual(await createCollection(browser, '$dropFetch'), { id: 'first' });
      assert.deepEqual(await createCollection(browser, '$apiFetch'), { id: 'second' });
      assert.equal(overlappingClient.calls.length, 0);
      await finishHydration(overlappingClient);
    });

    const failedSlot = makeContext('failed-slot');
    failedSlot.fetch = () => {
      if (failedSlot.calls.length === 1) throw new Error('Synthetic mutation failure');
      return { id: 'after-failure' };
    };
    await contexts.run(failedSlot, async () => {
      await assert.rejects(createCollection(server, '$dropFetch'), /Synthetic mutation failure/);
      assert.deepEqual(await createCollection(server, '$apiFetch'), { id: 'after-failure' });
      assert.deepEqual(failedSlot.state.get('api-fetch-transfer').value.map((entry) => entry.ready), [false, true]);
    });
    const failedSlotClient = makeContext('failed-slot-browser');
    failedSlotClient.state = structuredClone(failedSlot.state);
    failedSlotClient.app.isHydrating = true;
    await contexts.run(failedSlotClient, async () => {
      // A missing response cannot shift the next invocation's successful slot.
      assert.deepEqual(await createCollection(browser, '$apiFetch'), failedSlotClient.response);
      assert.deepEqual(await createCollection(browser, '$dropFetch'), { id: 'after-failure' });
      assert.equal(failedSlotClient.calls.length, 1);
      await finishHydration(failedSlotClient);
    });

    const failedRead = makeContext('failed-read');
    await contexts.run(failedRead, async () => {
      failedRead.error = new Error('Synthetic read failure');
      await assert.rejects(server.$apiFetch('GET /api/v1'), /Synthetic read failure/);
      failedRead.error = undefined;
      assert.deepEqual(await server.$dropFetch('/api/v1', { responseType: 'json' }), failedRead.response);
      assert.deepEqual(await server.$apiFetch('GET /api/v1'), failedRead.response);
      assert.equal(failedRead.calls.length, 2);
    });

    const action = makeContext('outside-setup');
    action.instance = undefined;
    await contexts.run(action, async () => {
      await server.$dropFetch('/api/v1/user/mfa/totp/start', { method: 'POST' });
      await server.$dropFetch('/api/v1/user/mfa/totp/start', { method: 'POST' });
      assert.equal(action.calls.length, 2);
      assert.equal(action.state.get('api-fetch-transfer').value.length, 2);
    });

    for (const value of [false, null, undefined, 0, '']) {
      const context = makeContext('falsy');
      context.response = value;
      await contexts.run(context, async () => {
        await server.$dropFetch('/api/v1', { responseType: 'json' });
        assert.equal(await server.$apiFetch('GET /api/v1'), value);
        assert.equal(context.calls.length, 1);
      });
    }
    const legacy = makeContext('legacy', { cookie: 'session=sample-legacy' });
    await contexts.run(legacy, async () => {
      const options = {
        method: 'PATCH',
        params: { id: 'sample-game', filter: 'sample-filter' },
        query: { sort: 'name' },
        body: { mName: 'Sample' },
        headers: { Authorization: 'Bearer sample-legacy' },
      };
      assert.deepEqual(await server.$dropFetch('/api/v1/admin/game/:id', options), legacy.response);
      assert.equal(legacy.calls[0].url, '/api/v1/admin/game/sample-game');
      assert.equal(legacy.calls[0].options.method, 'PATCH');
      assert.deepEqual(legacy.calls[0].options.params, { filter: 'sample-filter' });
      assert.deepEqual(legacy.calls[0].options.query, { sort: 'name' });
      assert.deepEqual(legacy.calls[0].options.body, { mName: 'Sample' });
      assert.equal(legacy.calls[0].options.headers.get('cookie'), 'session=sample-legacy');
      assert.equal(legacy.calls[0].options.headers.get('authorization'), 'Bearer sample-legacy');
      assert.deepEqual(options.params, { id: 'sample-game', filter: 'sample-filter' });
    });
    const path = { id: 'sample/with %?#ü' };
    await contexts.run(a, async () => {
      const calls = a.calls.length;
      for (const id of ['.', '..']) {
        assert.throws(() => server.$apiFetch('GET /api/v1/games/:id', { path: { id } }), /Invalid path parameter/);
      }
      for (const hook of ['onRequest', 'onRequestError', 'onResponse', 'onResponseError']) {
        assert.throws(() => server.$apiFetch('GET /api/v1', {
          [hook]: ({ options }) => { options.headers.set('authorization', 'Bearer sample-other'); },
        }), /Unsupported typed request option/);
      }
      assert.equal(a.calls.length, calls);
      await server.$apiFetch('GET /api/v1/games/:id', { path });
      assert.equal(a.calls.at(-1).url, '/api/v1/games/sample%2Fwith%20%25%3F%23%C3%BC');
      assert.deepEqual(path, { id: 'sample/with %?#ü' });
      a.error = new Error('Synthetic failure');
      await assert.rejects(server.$apiFetch('GET /api/v1/admin/search/game', { query: { q: 'sample' } }), /Synthetic failure/);
    });
  } finally {
    for (const [name, descriptor] of originals) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else delete globalThis[name];
    }
  }
});
