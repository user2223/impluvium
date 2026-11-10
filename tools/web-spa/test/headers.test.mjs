import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { createRequire } from 'node:module';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import test from 'node:test';
import { compiler } from '../../web-api/compiler.mjs';
import { rootDirectory as root } from '../../web-api/generate.mjs';
import { hashBytes, spaSecurityHeaders } from '../../../server/server/utils/spa-contract.ts';

test('early redirects receive all replacement headers and HTML keeps its build hashes', async () => {
  const require = createRequire(join(root, 'server/package.json'));
  const h3 = require('h3');
  const hooks = new Map();
  const originals = new Map();
  const install = (name, value) => {
    originals.set(name, Object.getOwnPropertyDescriptor(globalThis, name));
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value });
  };
  install('defineNitroPlugin', (plugin) => plugin);
  for (const name of ['setHeaders', 'setHeader', 'removeResponseHeader']) install(name, h3[name]);
  let server;
  try {
    const source = await readFile(join(root, 'server/server/plugins/00.spa-headers.ts'), 'utf8');
    const ts = compiler(root);
    const code = ts.transpileModule(source, {
      compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext },
    }).outputText.replaceAll('import.meta.dev', 'false').replaceAll('"../utils/spa-contract"',
      JSON.stringify(pathToFileURL(join(root, 'server/server/utils/spa-contract.ts')).href));
    const { default: plugin } = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
    plugin({ hooks: { hook(name, callback) { hooks.set(name, callback); } } });
    assert.equal(typeof hooks.get('request'), 'function');
    assert.equal(typeof hooks.get('beforeResponse'), 'function');
    const app = h3.createApp({
      onRequest: (event) => hooks.get('request')(event),
      onBeforeResponse: (event) => hooks.get('beforeResponse')(event),
    });
    let deliveries = 0;
    // Model Nitro's ordering: route rules return before project middleware.
    app.use(h3.eventHandler((event) => {
      if (event.path.split('?')[0] !== '/auth/callback/oidc') return;
      const query = event.path.slice(event.path.indexOf('?'));
      return h3.sendRedirect(event, '/api/v1/auth/oidc/callback' + query, 307);
    }));
    const hash = `'sha256-${hashBytes('window.synthetic = true;')}'`;
    app.use(h3.eventHandler((event) => {
      deliveries++;
      h3.setHeader(event, 'Content-Security-Policy', spaSecurityHeaders([hash])['Content-Security-Policy']);
      h3.setHeader(event, 'X-Powered-By', 'Synthetic renderer');
      return 'Synthetic HTML';
    }));
    server = createServer(h3.toNodeListener(app));
    await new Promise((accept) => server.listen(0, '127.0.0.1', accept));
    const origin = `http://127.0.0.1:${server.address().port}`;
    const query = '?code=synthetic&state=synthetic%2Fstate';
    const redirect = await fetch(origin + '/auth/callback/oidc' + query, { redirect: 'manual' });
    assert.equal(redirect.status, 307);
    assert.equal(deliveries, 0, 'The project delivery middleware must be bypassed for this probe');
    assert.equal(redirect.headers.get('location'), '/api/v1/auth/oidc/callback' + query);
    for (const [name, value] of Object.entries(spaSecurityHeaders([]))) {
      assert.equal(redirect.headers.get(name), value, name);
    }
    assert.equal(redirect.headers.get('X-Powered-By'), null);
    await redirect.text();
    const page = await fetch(origin + '/store');
    assert.equal(page.status, 200);
    assert.ok(page.headers.get('Content-Security-Policy').includes(hash));
    assert.equal(page.headers.get('X-Powered-By'), null);
    await page.text();
  } finally {
    if (server) {
      server.closeAllConnections();
      await new Promise((accept, reject) => server.close((error) => error ? reject(error) : accept()));
    }
    for (const [name, descriptor] of originals) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else delete globalThis[name];
    }
  }
});
