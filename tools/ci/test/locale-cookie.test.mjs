import test from 'node:test';
import assert from 'node:assert/strict';
import {
  LEGACY_UI_LOCALE_COOKIE,
  UI_LOCALE_COOKIE,
  decodeLocaleCookie,
  legacyUiLocale,
  malformedLocaleCookies,
} from '../../../server/utils/locale-cookie.ts';

const locales = ['en-us', 'en-gb', 'de', 'fr'];

test('locale cookie: malformed percent-encoding is discarded instead of throwing', () => {
  assert.equal(decodeLocaleCookie('%E0%A4%A'), undefined);
  assert.equal(decodeLocaleCookie('%'), undefined);
  assert.equal(decodeLocaleCookie('de'), 'de');
  assert.equal(decodeLocaleCookie('en%2Dus'), 'en-us');
});

test('locale cookie: only locale cookies with undecodable values are reported as malformed', () => {
  assert.deepEqual(malformedLocaleCookies(''), []);
  assert.deepEqual(malformedLocaleCookies(`${UI_LOCALE_COOKIE}=de; ${LEGACY_UI_LOCALE_COOKIE}=fr; other=%`), []);
  assert.deepEqual(malformedLocaleCookies(`${UI_LOCALE_COOKIE}=%`), [UI_LOCALE_COOKIE]);
  assert.deepEqual(malformedLocaleCookies(`a=b; ${LEGACY_UI_LOCALE_COOKIE}=%E0%A4%A`), [LEGACY_UI_LOCALE_COOKIE]);
  assert.deepEqual(malformedLocaleCookies(`${UI_LOCALE_COOKIE}=%; ${LEGACY_UI_LOCALE_COOKIE}=%`).sort(), [LEGACY_UI_LOCALE_COOKIE, UI_LOCALE_COOKIE].sort());
});

test('locale cookie: a supported legacy preference is imported only when the new cookie is absent', () => {
  for (const absent of [undefined, null]) {
    assert.equal(legacyUiLocale(absent, 'de', locales), 'de');
    assert.equal(legacyUiLocale(absent, 'en-gb', locales), 'en-gb');
  }
});

test('locale cookie: the current cookie always takes precedence over the legacy preference', () => {
  for (const current of ['en-us', 'fr', '', 'unsupported', false, 0]) {
    assert.equal(legacyUiLocale(current, 'de', locales), undefined);
  }
});

test('locale cookie: unsupported and malformed legacy values are never imported', () => {
  for (const legacy of [undefined, null, '', 'DE', 'de-DE', 'unsupported', 1, true, ['de'], { locale: 'de' }]) {
    assert.equal(legacyUiLocale(undefined, legacy, locales), undefined);
  }
});

test('locale cookie: an imported preference cannot be applied again over a later selection', () => {
  const imported = legacyUiLocale(undefined, 'de', locales);
  assert.equal(imported, 'de');
  assert.equal(legacyUiLocale(imported, 'fr', locales), undefined);
  assert.equal(legacyUiLocale('en-us', 'de', locales), undefined);
});
