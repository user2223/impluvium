import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { canonicalSpaPath, createSpaManifest, hashBytes, inlineHashes, isServerPath, matchesApiRoute, spaCacheControl, spaFile, spaSecurityHeaders, validateSpaManifest } from '../../../server/server/utils/spa-contract.ts';
import { apiRoutes } from '../../../server/shared/api/generated/routes.ts';

const tools = dirname(dirname(fileURLToPath(import.meta.url)));
const html = '<!doctype html><html><head><link rel="stylesheet" href="/loading.css"></head><body><script type="module">window.synthetic = true;</script><script type="importmap">{"imports":{}}</script><script type="application/json">{"synthetic":true}</script></body></html>';

async function fixture(run, document = html) {
  const directory = await mkdtemp(join(tmpdir(), 'static-output-'));
  try {
    const output = join(directory, 'public');
    await mkdir(output);
    await writeFile(join(output, 'index.html'), document);
    await writeFile(join(output, 'loading.css'), 'body { background: #09090b; }');
    const manifest = await createSpaManifest(output);
    await writeFile(join(output, 'csp-hashes.json'), JSON.stringify(manifest));
    await run(output, manifest, directory);
  } finally { await rm(directory, { recursive: true, force: true }); }
}

test('the manifest binds HTML/assets and grants hashes only to executable inline scripts', async () => {
  await fixture(async (output, manifest) => {
    assert.deepEqual(await validateSpaManifest(output), manifest);
    assert.deepEqual(manifest.scripts['index.html'], [
      `'sha256-${hashBytes('window.synthetic = true;')}'`,
      `'sha256-${hashBytes('{"imports":{}}')}'`,
    ].sort());
    assert.equal(manifest.files['index.html'].size, Buffer.byteLength(html));
    assert.deepEqual(inlineHashes('<script src="/external.js"></script><script>one\r\ntwo</script>'),
      [`'sha256-${hashBytes('one\ntwo')}'`]);
    await writeFile(join(output, '200.html'), '<script>window.second = 1;</script>');
    await assert.rejects(validateSpaManifest(output), /does not match/);
    const updated = await createSpaManifest(output);
    assert.ok(updated.scripts['200.html'].length);
    assert.ok(updated.files['200.html']);
  });
});

test('inline styles and attributes fail the static HTML contract', () => {
  for (const html of ['<style>body{color:red}</style>', '<p style="color:red">Sample</p>']) {
    assert.throws(() => inlineHashes(html), /inline styles/);
  }
});

for (const mutation of ['missing', 'invalid-json', 'schema', 'script-hash', 'wrong-hash', 'html', 'data-body', 'data-hash', 'asset', 'missing-file', 'extra-file']) {
  test(`startup rejects ${mutation} and never reaches the Nitro entry point`, async () => {
    await fixture(async (output, manifest, directory) => {
      const server = join(directory, 'server');
      await mkdir(server);
      const preflight = await readFile(join(tools, 'preflight.mjs'), 'utf8');
      await writeFile(join(server, 'index.mjs'), preflight + '\nconsole.log("NITRO_IMPORT_REACHED");\n');
      const execute = () => new Promise((accept, reject) => {
        const child = spawn(process.execPath, [join(server, 'index.mjs')], { timeout: 10000 });
        let output = '';
        child.stdout.on('data', (bytes) => { output += bytes; });
        child.stderr.on('data', (bytes) => { output += bytes; });
        child.on('error', reject);
        child.on('close', (code, signal) => signal ? reject(new Error(`Preflight terminated: ${signal}`)) :
          accept({ code, output }));
      });
      // Positive control: the same entry point reaches its continuation.
      const healthy = await execute();
      assert.equal(healthy.code, 0, healthy.output);
      assert.match(healthy.output, /NITRO_IMPORT_REACHED/);
      const file = join(output, 'csp-hashes.json');
      if (mutation === 'missing') await rm(file);
      if (mutation === 'invalid-json') await writeFile(file, '{');
      if (mutation === 'schema') await writeFile(file, JSON.stringify({ ...manifest, version: 2 }));
      if (mutation === 'script-hash') {
        manifest.scripts['index.html'] = ['invalid-hash'];
        await writeFile(file, JSON.stringify(manifest));
      }
      if (mutation === 'wrong-hash') {
        manifest.scripts['index.html'][0] = `'sha256-${hashBytes('different-script')}'`;
        await writeFile(file, JSON.stringify(manifest));
      }
      if (mutation === 'html') await writeFile(join(output, 'index.html'), html.replace('true', 'false'));
      if (mutation === 'data-body') {
        const changed = html.replace('{"synthetic":true}', '{"synthetic":false}');
        assert.notEqual(changed, html);
        assert.deepEqual(inlineHashes(changed), manifest.scripts['index.html']);
        await writeFile(join(output, 'index.html'), changed);
      }
      if (mutation === 'data-hash') {
        manifest.scripts['index.html'].push(`'sha256-${hashBytes('{"synthetic":true}')}'`);
        manifest.scripts['index.html'].sort();
        await writeFile(file, JSON.stringify(manifest));
      }
      if (mutation === 'asset') await writeFile(join(output, 'loading.css'), 'body{}');
      if (mutation === 'missing-file') await rm(join(output, 'loading.css'));
      if (mutation === 'extra-file') await writeFile(join(output, 'extra.js'), 'window.extra = true;');
      const broken = await execute();
      assert.notEqual(broken.code, 0, broken.output);
      assert.doesNotMatch(broken.output, /NITRO_IMPORT_REACHED/);
      await assert.rejects(validateSpaManifest(output));
    });
  });
}

test('well-formed but incorrect script hashes fail the full startup validator', async () => {
  await fixture(async (output, manifest) => {
    manifest.scripts['index.html'][0] = `'sha256-${hashBytes('different-script')}'`;
    await writeFile(join(output, 'csp-hashes.json'), JSON.stringify(manifest));
    await assert.rejects(validateSpaManifest(output), /does not match/);
  });
});

test('path classification and lookup use the same canonical path', () => {
  for (const [raw, canonical] of [
    ['//_nuxt/missing', '/_nuxt/missing'], ['/_nuxt/./missing', '/_nuxt/missing'],
    ['/./_nuxt/missing', '/_nuxt/missing'], ['/%2e/_nuxt/missing', '/_nuxt/missing'],
    ['//store/x', '/store/x'], ['/store/a.b', '/store/a.b'],
    ['/auth/callback/oidc?code=synthetic%2Fvalue', '/auth/callback/oidc'],
  ]) assert.equal(canonicalSpaPath(raw), canonical);
  for (const raw of ['/_nuxt/missing%252Ejs', '/missing%252Ejs', '/%255Fnuxt/missing',
    '/missing.js;v=1', '/_nuxt/%2E%2E/csp-hashes.json', '/a%2fb', '/a%5cb',
    '/a\\b', '/%00', '/%', '/%E0%A4%A', '/../index.html']) {
    assert.throws(() => canonicalSpaPath(raw), undefined, raw);
  }
  for (const path of ['/api', '/api/v1/depot/x', '/api/v1/task', '/api/v1/notifications/ws',
    '/api/v1/admin/system-data/ws', '/auth/oidc', '/auth/signout', '/auth/callback/oidc']) {
    assert.equal(isServerPath(path), true, path);
  }
  assert.equal(isServerPath('/auth/signin'), false);
  assert.equal(isServerPath('/apiary'), false);
});

test('static paths reject every literal or encoded C0 control and DEL', () => {
  for (const code of [...Array.from({ length: 32 }, (_, index) => index), 0x7f]) {
    const character = String.fromCharCode(code);
    const encoded = `%${code.toString(16).padStart(2, '0')}`;
    for (const path of [`/a${character}b`, `/a${encoded}b`]) {
      assert.throws(() => canonicalSpaPath(path), /Invalid request path/, `U+${code.toString(16)} must be rejected`);
    }
  }
  for (const character of [' ', '~', '\u0080', 'ä', '😀']) {
    const path = `/a${character}b`;
    assert.equal(canonicalSpaPath(path), path);
    assert.equal(canonicalSpaPath(`/a${encodeURIComponent(character)}b`), path);
  }
});

test('strict headers allow the measured script hashes without eval or inline styles', () => {
  const hashes = inlineHashes(html);
  const headers = spaSecurityHeaders(hashes);
  const csp = headers['Content-Security-Policy'];
  for (const hash of hashes) assert.ok(csp.includes(hash));
  assert.doesNotMatch(csp, /unsafe-inline|unsafe-eval/);
  for (const directive of ["style-src 'self'", "connect-src 'self'", "object-src 'none'",
    "script-src-attr 'none'", "style-src-attr 'none'"]) assert.ok(csp.includes(directive));
  assert.equal(headers['X-Content-Type-Options'], 'nosniff');
  const expected = {
    'Referrer-Policy': 'no-referrer',
    'Permissions-Policy': 'camera=(), display-capture=(), fullscreen=(), geolocation=(), microphone=()',
    'Cross-Origin-Resource-Policy': 'same-origin',
    'Cross-Origin-Opener-Policy': 'same-origin',
    'Cross-Origin-Embedder-Policy': 'credentialless',
    'Origin-Agent-Cluster': '?1',
    'X-Content-Type-Options': 'nosniff',
    'X-DNS-Prefetch-Control': 'off',
    'X-Download-Options': 'noopen',
    'X-Frame-Options': 'SAMEORIGIN',
    'X-Permitted-Cross-Domain-Policies': 'none',
    'X-XSS-Protection': '0',
  };
  for (const [name, value] of Object.entries(expected)) assert.equal(headers[name], value, name);
  assert.equal(headers['Strict-Transport-Security'], undefined, 'The existing configuration disabled HSTS');
  assert.ok(csp.includes("base-uri 'none'"));
  assert.doesNotMatch(csp, /upgrade-insecure-requests/);
});

test('missing assets never receive the shell; deep links and dotted client routes do', () => {
  const files = { 'index.html': {}, '_nuxt/entry.samplehash.js': {} };
  for (const raw of ['/missing.JS?v=1', '//_nuxt/missing', '/_nuxt/./missing',
    '/./_nuxt/missing', '/%2e/_nuxt/missing', '/_fonts/', '/missing.png', '/csp-hashes.json']) {
    assert.equal(spaFile(canonicalSpaPath(raw), files), undefined, raw);
  }
  for (const raw of ['/admin/users', '//store/x', '/store/a.b', '/unknown-client-page']) {
    assert.equal(spaFile(canonicalSpaPath(raw), files), 'index.html', raw);
  }
  assert.equal(spaFile('/_nuxt/entry.samplehash.js', files), '_nuxt/entry.samplehash.js');
});

test('API misses stay errors and encoded API parameters remain owned by Nitro', () => {
  const routes = Object.values(apiRoutes);
  for (const path of ['/api', '/api/v1/unknown-handler']) assert.equal(matchesApiRoute(path, 'GET', routes), false);
  assert.equal(matchesApiRoute('/api/v1/object/a%2Fb%20c', 'GET', routes), true);
  assert.equal(matchesApiRoute('/api/v1/user', 'HEAD', routes), true);
  assert.equal(matchesApiRoute('/api/v1/user', 'DELETE', routes), false);
  assert.equal(matchesApiRoute('/api/v1/task', 'GET', routes), true);
  assert.equal(matchesApiRoute('/api/v1/auth/oidc/callback', 'GET', routes), true);
});

test('hash-only and named hashed build assets are immutable while HTML and metadata revalidate', () => {
  for (const file of ['_nuxt/i2pVpLqz.js', '_nuxt/i2pVpLqz.js.map', '_nuxt/i2pVpLqz.css',
    '_nuxt/entry.CVxyz123.js', '_nuxt/font.abcdefgh.woff2']) {
    assert.equal(spaCacheControl(file), 'public, max-age=31536000, immutable', file);
  }
  for (const file of ['index.html', '200.html', 'loading.css', '_nuxt/manifest.json',
    '_nuxt/entry.js', 'fonts/InterVariable.ttf']) assert.equal(spaCacheControl(file), 'no-cache', file);
});

test('build, full validation and preflight agree on script types, including import maps', async () => {
  const cases = [
    ['', 'window.classic = 1;', true], ['type=""', 'window.emptyType = 1;', true],
    ['type=module', 'window.moduleType = 1;', true],
    ['type="importmap"', '{"imports":{}}', true],
    ['type="MODULE"', 'window.uppercaseModule = 1;', true],
    ['type="IMPORTMAP"', '{"imports":{"synthetic":"/module.js"}}', true],
    ['TYPE=" TEXT/JAVASCRIPT "', 'window.mime = 1;', true],
    ['type="application/ecmascript"', 'window.ecma = 1;', true],
    ['type="text/x-javascript"', 'window.legacyMime = 1;', true],
    ['type="text/javascript1.5"', 'window.legacyVersion = 1;', true],
    ['language=javascript', 'window.language = 1;', true],
    ['type="mod&#117;le"', 'window.entity = 1;', true],
    ['type="text&sol;javascript"', 'window.namedEntity = 1;', true],
    ['type="application/json"', '{"data":1}', false],
    ['type="application/ld+json"', '{"data":2}', false],
    ['type="speculationrules"', '{"prefetch":[]}', false],
    ['type="text/plain"', 'plain data', false],
    ['type="\u00a0text/javascript"', 'non-ASCII whitespace data', false],
    ['language=unknown', 'unknown language data', false],
    ['src=""', 'ignored inline source', false],
    ['SRC="/external.js"', 'ignored external source', false],
    ['type="application/json" type="module"', '{"firstTypeWins":true}', false],
  ];
  const document = cases.map(([attributes, body]) => `<script ${attributes}>${body}</script>`).join('');
  const expected = cases.filter(([, , executable]) => executable).map(([, body]) => `'sha256-${hashBytes(body)}'`).sort();
  assert.deepEqual(inlineHashes(document), expected);
  await fixture(async (output, manifest, directory) => {
    assert.deepEqual(manifest.scripts['index.html'], expected);
    assert.deepEqual(await validateSpaManifest(output), manifest);
    const server = join(directory, 'server');
    await mkdir(server);
    await writeFile(join(server, 'index.mjs'), await readFile(join(tools, 'preflight.mjs'), 'utf8'));
    const code = await new Promise((accept, reject) => {
      const child = spawn(process.execPath, [join(server, 'index.mjs')], { stdio: 'pipe', timeout: 10000 });
      let log = '';
      child.stderr.on('data', (bytes) => { log += bytes; });
      child.on('error', reject);
      child.on('close', (code, signal) => signal ? reject(new Error(`Preflight terminated: ${signal}`)) :
        code === 0 ? accept(code) : reject(new Error(log)));
    });
    assert.equal(code, 0);
  }, document);
});
