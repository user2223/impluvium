import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import test from 'node:test';
import { compiler } from '../../web-api/compiler.mjs';
import { rootDirectory as root } from '../../web-api/generate.mjs';
import { UI_LOCALE_COOKIE, LEGACY_UI_LOCALE_COOKIE, decodeLocaleCookie } from '../../../server/utils/locale-cookie.ts';

async function loadSource(file) {
  const ts = compiler(root);
  let code = ts.transpileModule(await readFile(join(root, file), 'utf8'), {
    compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext },
  }).outputText;
  code = code.replaceAll('"~/utils/locale-cookie"', JSON.stringify(pathToFileURL(join(root, 'server/utils/locale-cookie.ts')).href));
  return import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
}

async function fixture(cookies, check) {
  const originals = new Map();
  const install = (name, value) => {
    originals.set(name, Object.getOwnPropertyDescriptor(globalThis, name));
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value });
  };
  const jar = new Map(Object.entries(cookies));
  const events = [];
  const heads = [];
  const locales = [
    { code: 'en-us', language: 'en-us' },
    { code: 'de', language: 'de' },
    { code: 'fr', language: 'fr' },
    { code: 'ar', language: 'ar', dir: 'rtl' },
  ];
  const localeCodes = locales.map(({ code }) => code);
  const locale = { value: 'en-us' };
  const localeProperties = {
    get value() { return locales.find(({ code }) => code === locale.value) ?? { code: locale.value }; },
  };
  let hook;
  const writes = [];
  install('document', {
    get cookie() { return [...jar].map(([name, value]) => `${name}=${value}`).join('; '); },
    set cookie(value) {
      const name = value.slice(0, value.indexOf('='));
      events.push(`expire:${name}`);
      assert.match(value, /; Max-Age=0; Path=\/; SameSite=Lax$/);
      jar.delete(name);
    },
  });
  install('defineNuxtPlugin', (definition) => definition);
  install('unref', (value) => value.value);
  install('useCookie', (name, options) => {
    events.push(`read:${name}`);
    assert.equal(typeof options.decode, 'function');
    return {
      get value() { return jar.has(name) ? options.decode(jar.get(name)) : undefined; },
      set value(value) {
        assert.ok(!options.readonly, 'The captured current preference must be read-only');
        if (value === null) jar.delete(name);
        else jar.set(name, encodeURIComponent(value));
      },
    };
  });
  install('useLocaleHead', () => assert.fail('HTML attributes must not invoke SEO head generation'));
  install('useHead', (getter) => { assert.equal(typeof getter, 'function'); heads.push(getter); });
  try {
    const { useLocaleHtmlAttributes } = await loadSource('server/composables/locale-head.ts');
    install('useLocaleHtmlAttributes', useLocaleHtmlAttributes);
    const { default: plugin } = await loadSource('server/plugins/00.locale.ts');
    assert.equal(plugin.order, -30, 'Cookie cleanup and hook registration must precede the router');
    const app = {
      hook(name, callback) { assert.equal(name, 'app:created'); hook = callback; },
      runWithContext(callback) { return callback(); },
      $i18n: {
        locale,
        localeProperties,
        localeCodes: { value: localeCodes },
        async setLocale(code) { events.push(`locale:${code}`); await Promise.resolve(); locale.value = code; },
        setLocaleCookie(locale) { writes.push(locale); jar.set(UI_LOCALE_COOKIE, encodeURIComponent(locale)); },
      },
    };
    install('useNuxtApp', () => app);
    const run = async (browserLocale = 'en-us') => {
      hook = undefined;
      plugin.setup(app);
      assert.equal(typeof hook, 'function');
      assert.equal(heads.length, 0, 'Language attributes bind after i18n initialization');
      // Simulate i18n's own cookie/browser selection between plugin setup and
      // app:created, including its creation of a default preference cookie.
      const current = decodeLocaleCookie(jar.get(UI_LOCALE_COOKIE) ?? '');
      const detected = localeCodes.includes(current) ? current : browserLocale;
      jar.set(UI_LOCALE_COOKIE, encodeURIComponent(detected));
      locale.value = detected;
      await hook();
      assert.equal(heads.length, 1);
    };
    await check({ jar, events, heads, writes, locale, locales, run });
  } finally {
    for (const [name, descriptor] of originals) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else delete globalThis[name];
    }
  }
}

test('the browser locale plugin imports a legacy preference once before app setup', async () => {
  await fixture({ [LEGACY_UI_LOCALE_COOKIE]: 'de' }, async ({ jar, heads, writes, run }) => {
    await run('en-us');
    assert.deepEqual(writes, ['de']);
    assert.equal(jar.get(UI_LOCALE_COOKIE), 'de');
    assert.equal(jar.has(LEGACY_UI_LOCALE_COOKIE), false);
    assert.deepEqual(heads[0](), { htmlAttrs: { lang: 'de', dir: 'ltr' } });
    heads.length = 0;
    await run('fr');
    assert.deepEqual(writes, ['de'], 'The legacy preference must not be imported again');
    assert.equal(jar.get(UI_LOCALE_COOKIE), 'de');
  });
});

test('the current browser cookie wins over the legacy cookie and browser language', async () => {
  await fixture({ [UI_LOCALE_COOKIE]: 'de', [LEGACY_UI_LOCALE_COOKIE]: 'fr' }, async ({ jar, writes, heads, run }) => {
    await run('en-us');
    assert.deepEqual(writes, []);
    assert.equal(jar.has(LEGACY_UI_LOCALE_COOKIE), false);
    assert.deepEqual(heads[0](), { htmlAttrs: { lang: 'de', dir: 'ltr' } });
  });
});

test('malformed browser cookies expire before Nuxt or i18n can decode them', async () => {
  for (const malformed of ['%', '%E0%A4%A']) {
    await fixture({ [UI_LOCALE_COOKIE]: malformed, [LEGACY_UI_LOCALE_COOKIE]: 'de', other: '%' },
      async ({ jar, events, writes, run }) => {
        await run();
        assert.equal(events[0], `expire:${UI_LOCALE_COOKIE}`);
        assert.deepEqual(writes, ['de']);
        assert.equal(jar.get('other'), '%');
        assert.equal(jar.has(LEGACY_UI_LOCALE_COOKIE), false);
      });
  }
  await fixture({ [UI_LOCALE_COOKIE]: '%', [LEGACY_UI_LOCALE_COOKIE]: '%E0%A4%A' },
    async ({ events, writes, heads, run }) => {
      await run('fr');
      assert.deepEqual(events.slice(0, 2).sort(), [`expire:${UI_LOCALE_COOKIE}`, `expire:${LEGACY_UI_LOCALE_COOKIE}`].sort());
      assert.deepEqual(writes, []);
      assert.deepEqual(heads[0](), { htmlAttrs: { lang: 'fr', dir: 'ltr' } });
    });
});

test('a present but unsupported current cookie never permits a legacy override', async () => {
  for (const current of ['', 'unsupported']) {
    await fixture({ [UI_LOCALE_COOKIE]: current, [LEGACY_UI_LOCALE_COOKIE]: 'de' },
      async ({ jar, writes, heads, run }) => {
        await run('fr');
        assert.deepEqual(writes, []);
        assert.equal(jar.has(LEGACY_UI_LOCALE_COOKIE), false);
        assert.deepEqual(heads[0](), { htmlAttrs: { lang: 'fr', dir: 'ltr' } });
      });
  }
});

test('absent or unsupported legacy cookies keep the browser selection', async () => {
  for (const cookies of [{}, { [LEGACY_UI_LOCALE_COOKIE]: 'unsupported' }]) {
    await fixture(cookies, async ({ jar, writes, heads, run }) => {
      await run('de');
      assert.deepEqual(writes, []);
      assert.equal(jar.has(LEGACY_UI_LOCALE_COOKIE), false);
      assert.deepEqual(heads[0](), { htmlAttrs: { lang: 'de', dir: 'ltr' } });
    });
  }
});

test('language attributes stay reactive and independent of layouts and API data', async () => {
  await fixture({}, async ({ locale, heads, run }) => {
    await run('en-us');
    assert.deepEqual(heads[0](), { htmlAttrs: { lang: 'en-us', dir: 'ltr' } });
    locale.value = 'de';
    assert.deepEqual(heads[0](), { htmlAttrs: { lang: 'de', dir: 'ltr' } });
    locale.value = 'ar';
    assert.deepEqual(heads[0](), { htmlAttrs: { lang: 'ar', dir: 'rtl' } });
    locale.value = 'en-us';
    assert.deepEqual(heads[0](), { htmlAttrs: { lang: 'en-us', dir: 'ltr' } });
    const errorRoot = await readFile(join(root, 'server/error.vue'), 'utf8');
    assert.match(errorRoot, /useLocaleHtmlAttributes\(\)/);
  });
});

test('language attributes preserve configured language tags and fall back to locale codes', async () => {
  await fixture({}, async ({ locale, locales, heads, run }) => {
    await run('de');
    const german = locales.find(({ code }) => code === 'de');
    german.language = 'de-DE';
    assert.deepEqual(heads[0](), { htmlAttrs: { lang: 'de-DE', dir: 'ltr' } });
    delete german.language;
    assert.deepEqual(heads[0](), { htmlAttrs: { lang: 'de', dir: 'ltr' } });
    locale.value = 'en-us';
    const english = locales.find(({ code }) => code === 'en-us');
    english.language = 'en-US';
    assert.deepEqual(heads[0](), { htmlAttrs: { lang: 'en-US', dir: 'ltr' } });
    delete english.language;
    assert.deepEqual(heads[0](), { htmlAttrs: { lang: 'en-us', dir: 'ltr' } });
  });
});
