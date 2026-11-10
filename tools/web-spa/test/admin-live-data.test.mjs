import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { join } from 'node:path';
import test from 'node:test';
import { rootDirectory as root } from '../../web-api/generate.mjs';

const require = createRequire(join(root, 'server/package.json'));
const { compile, createSSRApp, h } = require('vue');
const { parse } = require('vue/compiler-sfc');
const { renderToString } = require('vue/server-renderer');
const key = 'errors.liveDataUnavailable';

async function pageRenderer(file) {
  const source = await readFile(join(root, file), 'utf8');
  const { descriptor, errors } = parse(source);
  assert.deepEqual(errors, []);
  const template = descriptor.template.content;
  const render = compile(template, { onError: (error) => { throw error; } });
  const components = new Set([...template.matchAll(/<([A-Z][A-Za-z0-9]*|i18n-t)\b/g)].map((match) => match[1]));
  const translate = (key) => key;
  return async (status) => {
    const app = createSSRApp({
      render,
      setup: () => ({
        taskStatus: status, systemStatus: status,
        task: undefined, liveRunningTasks: [],
        historicalTasks: [{ id: 'completed', name: 'Completed HTTP task', success: true, actions: [] }],
        dailyTasks: [], weeklyTasks: [], other: [], scheduledTasks: {},
        systemData: { totalRam: 0, freeRam: 0, cpuLoad: 0, cpuCores: 0 },
        version: '0.0.0', gameCount: 0, sources: [], userStats: {}, pieChartData: [],
        t: translate, formatBytes: String, getBarColor: () => '', getPercentage: () => 0,
      }),
    });
    app.config.globalProperties.$t = translate;
    app.config.globalProperties.$n = String;
    for (const component of components) {
      app.component(component, { setup: (_, { slots }) => () => h('div', slots.default?.()) });
    }
    return renderToString(app);
  };
}

for (const [file, normalContent] of [
  ['server/pages/admin/task/[id]/index.vue', 'animate-spin'],
  ['server/pages/admin/task/index.vue', 'tasks.admin.noTasksRunning'],
  ['server/pages/admin/index.vue', 'home.admin.numberCores'],
]) {
  test(`${file} replaces unavailable live data with a local notice`, async () => {
    const render = await pageRenderer(file);
    const unavailable = await render('unavailable');
    assert.ok(unavailable.includes(key));
    assert.ok(unavailable.includes('role="status"'));
    assert.ok(!unavailable.includes(normalContent));
    if (file.endsWith('/task/index.vue')) {
      assert.ok(unavailable.includes('Completed HTTP task'), 'HTTP task history remains visible');
    }
    for (const status of ['connecting', 'connected', 'interrupted']) {
      const available = await render(status);
      assert.ok(!available.includes(key));
      assert.ok(available.includes(normalContent));
    }
  });
}

test('the local live data notice is translated in English and German', async () => {
  for (const [locale, expected] of [
    ['en_us', 'Live data is not available here in the desktop app. Please open this page in a browser.'],
    ['de', 'Live-Daten sind hier in der Desktop-App nicht verfügbar. Bitte öffne diese Seite im Browser.'],
  ]) {
    const messages = JSON.parse(await readFile(join(root, `server/i18n/locales/${locale}.json`), 'utf8'));
    assert.equal(messages.errors.liveDataUnavailable, expected);
  }
});
