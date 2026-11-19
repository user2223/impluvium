import assert from 'node:assert/strict';
import { readdir, readFile } from 'node:fs/promises';
import { join } from 'node:path';
import test from 'node:test';
import { rootDirectory as root } from '../../web-api/generate.mjs';
import { UNKNOWN_FLAG, flagCodepoints, localeFlag, localeFlagUrl } from '../../../server/utils/locale-flags.ts';

const app = join(root, 'server');
const source = (file) => readFile(join(app, file), 'utf8');

async function shippedLocales() {
  const config = await source('nuxt.config.ts');
  const block = config.slice(config.indexOf('locales: ['));
  const codes = [...block.matchAll(/code: "([^"]+)"/g)].map((match) => match[1]);
  assert.ok(codes.length >= 10, 'the language list was not found');
  return codes;
}

test('the language selection never loads a flag through the emoji route', async () => {
  const files = ['pages/setup.vue'];
  for (const name of await readdir(join(app, 'components/Selector'))) {
    if (name.endsWith('.vue')) files.push(`components/Selector/${name}`);
  }
  assert.ok(files.includes('components/Selector/LanguageListbox.vue'));
  for (const file of files) {
    const text = await source(file);
    assert.doesNotMatch(text, /\/api\/v1\/emoji/, file);
    assert.doesNotMatch(text, /EmojiText/, file);
  }
  const listbox = await source('components/Selector/LanguageListbox.vue');
  assert.match(listbox, /localeFlagUrl\(/);
});

test('every shipped language has its own static flag under /flags', async () => {
  for (const code of await shippedLocales()) {
    assert.notEqual(localeFlag(code), UNKNOWN_FLAG, `${code} has no flag`);
    assert.match(localeFlagUrl(code), /^\/flags\/[0-9a-f]{1,6}(?:-[0-9a-f]{1,6})*\.svg$/);
    assert.ok(flagCodepoints.includes(localeFlag(code)), code);
  }
  assert.equal(localeFlagUrl('not-a-language'), `/flags/${UNKNOWN_FLAG}.svg`);
  assert.deepEqual([...flagCodepoints], [...new Set(flagCodepoints)].sort());
});

test('the build copies the flags into the web application before the manifest is made', async () => {
  const build = await readFile(join(root, 'tools/web-spa/build.mjs'), 'utf8');
  assert.match(build, /flagCodepoints/);
  const copy = build.indexOf('await copyFlags(output)');
  const manifest = build.indexOf('await createSpaManifest(output)');
  assert.ok(copy !== -1 && manifest !== -1 && copy < manifest);
});
