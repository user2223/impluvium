import assert from 'node:assert/strict';

// Accept the browser fixture's Page; no browser runner is added to production
// dependencies. Run against a disposable instance of the built image.
export async function checkCsp(page) {
  await page.locator('#impluvium-loading').waitFor({ state: 'hidden' });
  const probeURL = new URL('/_nuxt/security-probe.js', page.url()).href;
  const source = `
    (async () => {
      const state = window.__securityProbe;
      state.started = true;
      const inline = document.createElement('script');
      inline.textContent = 'window.__securityProbe.inlineRan = true';
      document.head.append(inline);
      try { (0, eval)('window.__securityProbe.evalRan = true'); } catch {}
      const foreign = document.createElement('script');
      foreign.src = 'https://example.invalid/unapproved.js';
      const scriptDone = new Promise((resolve) => { foreign.onload = foreign.onerror = resolve; });
      document.head.append(foreign);
      const element = document.createElement('div');
      element.id = 'security-probe-element';
      document.body.append(element);
      const style = document.createElement('style');
      style.textContent = '#security-probe-element { outline-width: 17px; }';
      document.head.append(style);
      element.setAttribute('style', 'border-top-style: solid; border-top-width: 19px');
      const image = document.createElement('img');
      image.src = 'https://example.invalid/unapproved.png';
      const imageDone = new Promise((resolve) => { image.onload = image.onerror = resolve; });
      document.body.append(image);
      await Promise.all([scriptDone, imageDone,
        fetch('https://example.invalid/unapproved-fetch').catch(() => undefined)]);
      const computed = getComputedStyle(element);
      state.styleElementApplied = computed.outlineWidth === '17px';
      state.styleAttributeApplied = computed.borderTopWidth === '19px';
      state.done = true;
    })();
  `;
  await page.route(probeURL, (route) => route.fulfill({
    status: 200, contentType: 'text/javascript', body: source,
  }));
  try {
    await page.evaluate((url) => {
      window.__securityProbe = { violations: [] };
      document.addEventListener('securitypolicyviolation', (event) => {
        window.__securityProbe.violations.push({
          directive: event.effectiveDirective, target: event.blockedURI,
        });
      });
      const probe = document.createElement('script');
      probe.src = url;
      document.head.append(probe);
    }, probeURL);
    await page.waitForFunction(() => window.__securityProbe?.done, undefined, { timeout: 10000 });
    await page.waitForFunction(() => window.__securityProbe.violations.length >= 7, undefined, { timeout: 10000 });
    const state = await page.evaluate(() => window.__securityProbe);
    assert.equal(state.started, true, 'The permitted same-origin probe did not execute');
    for (const key of ['inlineRan', 'evalRan', 'styleElementApplied', 'styleAttributeApplied']) {
      assert.ok(!state[key], `CSP failed to block ${key}`);
    }
    for (const [directive, target] of [
      ['script-src-elem', 'inline'], ['script-src', 'eval'],
      ['script-src-elem', 'https://example.invalid'],
      ['style-src-elem', 'inline'], ['style-src-attr', 'inline'],
      ['connect-src', 'https://example.invalid'], ['img-src', 'https://example.invalid'],
    ]) {
      assert.ok(state.violations.some((event) => event.directive === directive &&
        event.target.startsWith(target)), `Missing effective CSP counterprobe: ${directive} ${target}`);
    }
    return state.violations;
  } finally {
    await page.unroute(probeURL);
    // Probe elements and violation listeners belong to this document only.
    await page.reload();
  }
}
