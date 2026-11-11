import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { loadWrapper } from '../../web-api/test/wrapper.mjs';
import { WebSocketHandler } from '../../../server/composables/ws.ts';
import { advanceSessionContext, confirmSessionContext } from '../../../server/composables/session-context.ts';
import { startNotificationConnection, useNotifications, useNotificationConnection } from '../../../server/composables/notifications.ts';
import { startTaskConnection, useTask, useTaskReady, useTaskConnection } from '../../../server/composables/task.ts';
import { startSystemConnection, useSystemData, useSystemConnection } from '../../../server/composables/admin-home.ts';
import { taskSocketFixture } from './server-fixture.mjs';

async function connectionFixture(run, protocol = 'https:', userAgent = 'Mozilla/5.0') {
  const originals = new Map();
  const install = (name, value) => {
    if (!originals.has(name)) originals.set(name, Object.getOwnPropertyDescriptor(globalThis, name));
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value });
  };
  const sockets = [];
  const timers = new Map();
  const scheduledTimers = [];
  const states = new Map();
  let timerId = 0;
  class Socket {
    static OPEN = 1;
    readyState = 0;
    sent = [];
    constructor(url) { this.url = url; sockets.push(this); }
    send(message) { this.sent.push(message); }
    close() { this.readyState = 3; this.onclose?.({ code: 1000 }); }
    open() { this.readyState = 1; this.onopen?.(); }
    message(data) { this.onmessage?.({ data }); }
  }
  install('WebSocket', Socket);
  install('location', { protocol, host: 'example.invalid', href: `${protocol}//example.invalid/store` });
  install('navigator', { userAgent });
  install('setTimeout', (callback, delay) => { const id = ++timerId; scheduledTimers.push(delay); timers.set(id, { callback, delay }); return id; });
  install('clearTimeout', (id) => { timers.delete(id); });
  install('ref', (value) => ({ value }));
  install('useState', (key, create) => {
    if (!states.has(key)) states.set(key, { value: create() });
    return states.get(key);
  });
  install('getCurrentScope', () => undefined);
  const pendingLists = [];
  install('$apiFetch', (key, options) => {
    assert.equal(key, 'GET /api/v1/notifications');
    assert.equal(options.timeout, 15000, 'Every snapshot attempt must have a deadline');
    assert.equal(options.signal.aborted, false);
    return new Promise((resolve, reject) => pendingLists.push({ resolve, reject, options }));
  });
  const disposers = [];
  const retry = () => {
    assert.equal(timers.size, 1);
    const [id, timer] = [...timers][0];
    timers.delete(id);
    timer.callback();
    return timer.delay;
  };
  try {
    advanceSessionContext();
    const { isClientRequest } = await loadWrapper();
    install('isClientRequest', isClientRequest);
    // Evaluate the app's actual computed value against the consumer states.
    const app = await readFile(new URL('../../../server/app.vue', import.meta.url), 'utf8');
    const warning = app.match(/const connectionInterrupted = computed\([\s\S]*?\n\);/);
    assert.ok(warning, 'The app must compute its live update warning');
    const interrupted = new Function('computed', 'notificationStatus', 'taskStatus', 'systemStatus',
      `${warning[0]}\nreturn connectionInterrupted;`)(
      (getter) => ({ get value() { return getter(); } }),
      useNotificationConnection(), useTaskConnection(), useSystemConnection(),
    );
    await run({ sockets, timers, scheduledTimers, pendingLists, disposers, retry, interrupted, install });
  } finally {
    for (const dispose of disposers) dispose();
    advanceSessionContext();
    for (const [name, descriptor] of originals) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else delete globalThis[name];
    }
  }
}

test('channels follow sessions, reconnect with capped backoff and stop on auth failure', async () => {
  await connectionFixture(async ({ sockets, timers, pendingLists, disposers, retry, interrupted }) => {
    let resets = 0;
    const statuses = [];
    const frames = [];
    const channel = new WebSocketHandler('/api/v1/task', (user) => user.admin,
      (status) => statuses.push(status), () => { resets++; });
    disposers.push(() => channel.dispose());
    channel.listen((frame) => frames.push(frame));
    channel.send('old-queued-message');
    assert.equal(sockets.length, 0);
    confirmSessionContext({ id: 'synthetic-user', admin: false });
    assert.equal(sockets.length, 0);
    confirmSessionContext({ id: 'synthetic-admin', admin: true });
    assert.equal(sockets.length, 1);
    const old = sockets.at(-1);
    assert.equal(old.url, 'wss://example.invalid/api/v1/task');
    old.open();
    assert.deepEqual(old.sent, []);
    confirmSessionContext({ id: 'synthetic-admin', admin: true });
    assert.equal(sockets.length, 1);
    // No successful application frame: retries grow even if a handshake opens.
    for (const expected of [1000, 2000, 4000, 8000, 16000, 30000, 30000]) {
      sockets.at(-1).close();
      assert.equal(statuses.at(-1), 'interrupted');
      assert.equal(retry(), expected);
      sockets.at(-1).open();
    }
    sockets.at(-1).message('current-frame');
    assert.deepEqual(frames, ['current-frame']);
    sockets.at(-1).message('unauthenticated');
    assert.equal(statuses.at(-1), 'unauthenticated');
    assert.equal(timers.size, 0);
    const count = sockets.length;
    confirmSessionContext({ id: 'synthetic-admin', admin: true });
    assert.equal(sockets.length, count);
    advanceSessionContext();
    old.message('old-frame');
    assert.deepEqual(frames, ['current-frame']);
    confirmSessionContext({ id: 'synthetic-admin', admin: true });
    assert.equal(sockets.length, count + 1);
    assert.ok(resets >= 3);
    channel.dispose();
    advanceSessionContext();

    // Exercise the production consumers, not only the socket wrapper.
    disposers.push(startNotificationConnection(), startTaskConnection(), startSystemConnection());
    confirmSessionContext({ id: 'synthetic-admin', admin: true });
    const notification = sockets.findLast((socket) => socket.url.endsWith('/notifications/ws'));
    const task = sockets.findLast((socket) => socket.url.endsWith('/task'));
    const system = sockets.findLast((socket) => socket.url.endsWith('/system-data/ws'));
    notification.open(); task.open(); system.open();
    const watched = useTask('synthetic-task');
    assert.deepEqual(task.sent, ['connect/synthetic-task']);
    task.message('connect');
    assert.equal(useTaskReady().value, true);
    task.message(JSON.stringify({ id: 'synthetic-task', log: ['first'], progress: 1 }));
    task.message(JSON.stringify({ id: 'synthetic-task', log: ['second'], progress: 2 }));
    assert.deepEqual(watched.value.log, ['first', 'second']);
    notification.message(JSON.stringify({ id: 'live', title: 'Live' }));
    pendingLists.at(-1).resolve([{ id: 'live', title: 'Snapshot' }, { id: 'missed', title: 'Missed' }]);
    await Promise.resolve();
    assert.deepEqual(useNotifications().value.map((value) => value.id).sort(), ['live', 'missed']);
    assert.equal(useNotifications().value.find((value) => value.id === 'live').title, 'Live');
    assert.equal(useNotificationConnection().value, 'connected');
    system.message(JSON.stringify({ totalRam: 100, freeRam: 20, cpuLoad: 5, cpuCores: 4 }));
    assert.equal(useSystemData().value.freeRam, 20);
    assert.equal(useSystemConnection().value, 'connected');

    task.close();
    retry();
    const recovered = sockets.at(-1);
    recovered.open();
    assert.equal(watched.value, undefined);
    assert.deepEqual(recovered.sent, ['connect/synthetic-task']);
    recovered.message(JSON.stringify({ id: 'synthetic-task', log: ['first', 'second', 'third'], progress: 3 }));
    assert.deepEqual(watched.value.log, ['first', 'second', 'third']);
    recovered.message('disconnect/synthetic-task');
    assert.equal(watched.value.progress, 3);
    assert.strictEqual(useTask('synthetic-task'), watched);
    recovered.close();
    retry();
    sockets.at(-1).open();
    assert.deepEqual(sockets.at(-1).sent, [], 'Completed tasks must not be resubscribed');
    assert.deepEqual(watched.value.log, ['first', 'second', 'third']);
    notification.close();
    retry();
    const recoveredNotification = sockets.at(-1);
    recoveredNotification.open();
    // An HTTP failure must recover even while the reopened socket stays open.
    const socketCount = sockets.length;
    for (const delay of [1000, 2000, 4000, 8000, 16000, 30000, 30000]) {
      pendingLists.at(-1).reject(Object.assign(new Error('Temporary failure'), { status: 503 }));
      await Promise.resolve();
      assert.equal(useNotificationConnection().value, 'interrupted');
      assert.equal(interrupted.value, true);
      assert.equal(retry(), delay);
      assert.equal(interrupted.value, true);
      assert.equal(sockets.length, socketCount);
    }
    recoveredNotification.message(JSON.stringify({ id: 'live', title: 'Recovered live' }));
    pendingLists.at(-1).resolve([{ id: 'live' }, { id: 'missed-during-outage' }]);
    await Promise.resolve();
    assert.equal(useNotificationConnection().value, 'connected');
    assert.deepEqual(useNotifications().value.map((item) => item.id).sort(), ['live', 'missed-during-outage']);
    assert.equal(timers.size, 0);
    recoveredNotification.close();
    retry();
    sockets.at(-1).open();
    pendingLists.at(-1).reject(Object.assign(new Error('Temporary failure'), { status: 503 }));
    await Promise.resolve();
    assert.equal(timers.size, 1);
    advanceSessionContext();
    assert.equal(timers.size, 0, 'Logout must cancel a scheduled list recovery');
    assert.equal(watched.value, undefined, 'Logout must clear completed task snapshots too');
    confirmSessionContext({ id: 'synthetic-admin', admin: true });
    sockets.findLast((socket) => socket.url.endsWith('/notifications/ws')).open();
    pendingLists.at(-1).reject(Object.assign(new Error('Unauthorized'), { status: 403 }));
    await Promise.resolve();
    assert.equal(useNotificationConnection().value, 'unauthenticated');
    assert.equal(timers.size, 0, 'Authentication failures must stop list retries');
    const denied = sockets.findLast((socket) => socket.url.endsWith('/notifications/ws'));
    const deniedCount = pendingLists.length;
    denied.close();
    retry();
    sockets.at(-1).open();
    assert.equal(pendingLists.length, deniedCount, 'A socket reconnect cannot retry a denied list before login');
    advanceSessionContext();
    confirmSessionContext({ id: 'synthetic-admin', admin: true });
    sockets.findLast((socket) => socket.url.endsWith('/notifications/ws')).open();
    const oldList = pendingLists.at(-1);
    advanceSessionContext();
    assert.equal(oldList.options.signal.aborted, true);
    assert.deepEqual(useNotifications().value, []);
    assert.equal(watched.value, undefined);
    assert.equal(useTaskReady().value, false);
    oldList.resolve([{ id: 'old-session', title: 'Old' }]);
    notification.message(JSON.stringify({ id: 'old-frame' }));
    await Promise.resolve();
    assert.deepEqual(useNotifications().value, []);
    confirmSessionContext({ id: 'synthetic-admin', admin: true });
    const newTask = sockets.findLast((socket) => socket.url.endsWith('/task'));
    newTask.open();
    assert.deepEqual(newTask.sent, []);
    const next = useTask('new-task');
    newTask.message(JSON.stringify({ id: 'new-task', log: ['new'], progress: 10 }));
    assert.deepEqual(next.value.log, ['new']);
  });
});

test('the task server authentication frame stops the task consumer until a new signin', async () => {
  const server = await taskSocketFixture();
  await connectionFixture(async ({ sockets, timers, scheduledTimers, disposers }) => {
    disposers.push(startTaskConnection());
    const session = { id: 'sample-admin', admin: true };
    confirmSessionContext(session);
    let socket = sockets.at(-1);
    socket.open();
    let peer = server.peer(new Headers(), (frame) => socket.message(frame));
    await server.route.open(peer);
    assert.equal(useTaskConnection().value, 'unauthenticated');
    assert.equal(useTaskReady().value, false);
    assert.equal(timers.size, 0);
    assert.deepEqual(scheduledTimers, []);
    confirmSessionContext(session);
    assert.equal(sockets.length, 1, 'Session confirmation cannot restart a blocked channel');

    advanceSessionContext();
    server.signIn();
    confirmSessionContext(session);
    assert.equal(sockets.length, 2, 'A new signin restarts the task channel');
    socket = sockets.at(-1);
    socket.open();
    peer = server.peer(new Headers(), (frame) => socket.message(frame));
    await server.route.open(peer);
    const task = useTask('sample-task');
    await server.route.message(peer, { text: () => socket.sent.at(-1) });
    assert.equal(task.value.error.title, 'Unknown task');
    assert.equal(useTaskConnection().value, 'connected', 'A task ACL denial is not an authentication failure');

    server.state.session = undefined;
    await server.route.message(peer, { text: () => 'connect/sample-task' });
    assert.equal(useTaskConnection().value, 'unauthenticated');
    assert.equal(useTaskReady().value, false);
    assert.equal(timers.size, 0);
    assert.deepEqual(scheduledTimers, [], 'Authentication failure must schedule zero reconnects');
    confirmSessionContext(session);
    assert.equal(sockets.length, 2);
    await server.route.close(peer);
  });
});

for (const [protocol, userAgent] of [
  ['server:', 'Mozilla/5.0'],
  ['file:', 'Mozilla/5.0'],
  ['http:', 'impluvium-desktop/0.0.0 (windows; x86_64)'],
]) {
  test(`${protocol} with ${userAgent} disables live updates until the session changes without scheduling retries`, async () => {
    await connectionFixture(async ({ sockets, timers, scheduledTimers, pendingLists, disposers, interrupted }) => {
      if (protocol === 'http:') {
        location.host = 'server.localhost';
        location.href = 'http://server.localhost/store';
      }
      disposers.push(startNotificationConnection(), startTaskConnection(), startSystemConnection());
      const session = { id: 'synthetic-admin', admin: true };
      const assertUnavailable = () => {
        assert.equal(sockets.length, 0);
        assert.equal(timers.size, 0);
        assert.deepEqual(scheduledTimers, [], 'Unavailable channels must never schedule a timer');
        assert.equal(useNotificationConnection().value, 'unavailable');
        assert.equal(useTaskConnection().value, 'unavailable');
        assert.equal(useSystemConnection().value, 'unavailable');
        assert.equal(useTaskReady().value, false);
        assert.deepEqual(useSystemData().value, { totalRam: 0, freeRam: 0, cpuLoad: 0, cpuCores: 0 });
        assert.equal(interrupted.value, false);
      };
      confirmSessionContext(session);
      assert.equal(useTask('unavailable-task').value, undefined);
      assertUnavailable();
      assert.equal(pendingLists.length, 1, 'Notifications still load one HTTP snapshot');
      assert.equal(pendingLists.at(-1).options.retry, false, 'The HTTP snapshot must disable ofetch retries');
      pendingLists.at(-1).resolve([{ id: 'snapshot', title: 'Snapshot' }]);
      await Promise.resolve();
      assert.deepEqual(useNotifications().value.map((item) => item.id), ['snapshot']);
      assertUnavailable();
      confirmSessionContext(session);
      assert.equal(pendingLists.length, 1);

      for (const status of [503, 401, 403]) {
        advanceSessionContext();
        confirmSessionContext(session);
        const count = pendingLists.length;
        assert.equal(pendingLists.at(-1).options.retry, false);
        pendingLists.at(-1).reject(Object.assign(new Error('Snapshot failed'), { status }));
        await Promise.resolve();
        confirmSessionContext(session);
        assertUnavailable();
        assert.equal(pendingLists.length, count, 'A failed snapshot must not retry in this session');
      }

      advanceSessionContext();
      confirmSessionContext(session);
      const obsolete = pendingLists.at(-1);
      advanceSessionContext();
      confirmSessionContext(session);
      assert.equal(obsolete.options.signal.aborted, true);
      obsolete.resolve([{ id: 'obsolete' }]);
      await Promise.resolve();
      assert.deepEqual(useNotifications().value, []);
      pendingLists.at(-1).resolve([{ id: 'current' }]);
      await Promise.resolve();
      assert.deepEqual(useNotifications().value.map((item) => item.id), ['current']);

      location.protocol = 'http:';
      navigator.userAgent = 'Mozilla/5.0';
      confirmSessionContext(session);
      assertUnavailable();
      advanceSessionContext();
      confirmSessionContext(session);
      assert.equal(sockets.length, 3, 'A new session must re-evaluate transport availability');
      assert.ok(sockets.every((socket) => socket.url.startsWith('ws://')));
      assert.equal(interrupted.value, false);
      assert.equal(timers.size, 0);
    }, protocol, userAgent);
  });
}

for (const [protocol, scheme] of [['http:', 'ws:'], ['https:', 'wss:']]) {
  test(`${protocol} only warns after a failure and throughout recovery`, async () => {
    await connectionFixture(async ({ sockets, timers, pendingLists, disposers, retry, interrupted }) => {
      disposers.push(startNotificationConnection(), startTaskConnection(), startSystemConnection());
      assert.equal(interrupted.value, false);
      confirmSessionContext({ id: 'synthetic-admin', admin: true });
      assert.equal(sockets.length, 3);
      assert.ok(sockets.every((socket) => socket.url.startsWith(`${scheme}//example.invalid/`)));
      assert.equal(useNotificationConnection().value, 'connecting');
      assert.equal(useTaskConnection().value, 'connecting');
      assert.equal(useSystemConnection().value, 'connecting');
      assert.equal(interrupted.value, false, 'The first connection attempt must not warn');
      assert.equal(timers.size, 0);

      const [notification, task, system] = sockets;
      notification.open(); system.open();
      assert.equal(pendingLists.at(-1).options.retry, undefined, 'Browser snapshot retries retain their default');
      assert.equal(interrupted.value, false, 'Initial snapshot recovery must not warn');
      pendingLists.at(-1).resolve([]);
      await Promise.resolve();
      const systemFrame = JSON.stringify({ totalRam: 100, freeRam: 20, cpuLoad: 5, cpuCores: 4 });
      system.message(systemFrame);

      task.close();
      assert.equal(interrupted.value, true, 'A failed first connection must warn');
      retry();
      assert.equal(interrupted.value, true);
      sockets.at(-1).open();
      assert.equal(interrupted.value, true, 'A handshake alone does not restore the channel');
      sockets.at(-1).message('connect');
      assert.equal(interrupted.value, false);

      sockets.at(-1).close();
      assert.equal(interrupted.value, true, 'A dropped established connection must warn');
      retry();
      assert.equal(interrupted.value, true);
      sockets.at(-1).open();
      sockets.at(-1).message('connect');
      assert.equal(interrupted.value, false);

      notification.close();
      assert.equal(interrupted.value, true);
      retry();
      sockets.at(-1).open();
      assert.equal(interrupted.value, true, 'The warning must remain until the HTTP snapshot returns');
      pendingLists.at(-1).resolve([]);
      await Promise.resolve();
      assert.equal(interrupted.value, false);

      system.onerror();
      assert.equal(interrupted.value, true);
      retry();
      sockets.at(-1).open();
      assert.equal(interrupted.value, true);
      sockets.at(-1).message(systemFrame);
      assert.equal(interrupted.value, false);
      sockets.at(-1).message('unauthenticated');
      assert.equal(interrupted.value, true);
      assert.equal(timers.size, 0);
      advanceSessionContext();
      assert.equal(interrupted.value, false);
      confirmSessionContext({ id: 'synthetic-admin', admin: true });
      assert.equal(interrupted.value, false, 'A new session starts without an interruption warning');
    }, protocol);
  });
}

test('a constructor failure warns until a retry restores application state', async () => {
  await connectionFixture(async ({ sockets, timers, disposers, retry, interrupted, install }) => {
    const Socket = globalThis.WebSocket;
    install('WebSocket', class {
      constructor() { throw new Error('Connection failed'); }
    });
    disposers.push(startTaskConnection());
    confirmSessionContext({ id: 'synthetic-admin', admin: true });
    assert.equal(sockets.length, 0);
    assert.equal(timers.size, 1);
    assert.equal(interrupted.value, true);
    install('WebSocket', Socket);
    assert.equal(retry(), 1000);
    assert.equal(interrupted.value, true);
    sockets.at(-1).open();
    sockets.at(-1).message('connect');
    assert.equal(interrupted.value, false);
  });
});

test('the interruption warning uses English and German messages from i18n', async () => {
  const app = await readFile(new URL('../../../server/app.vue', import.meta.url), 'utf8');
  assert.match(app, /\$t\("errors\.liveUpdatesInterrupted"\)/);
  for (const [locale, expected] of [
    ['en_us', 'Live updates are interrupted. Displayed values may be out of date.'],
    ['de', 'Die Live-Aktualisierung ist unterbrochen. Angezeigte Werte sind möglicherweise nicht mehr aktuell.'],
  ]) {
    const messages = JSON.parse(await readFile(new URL(`../../../server/i18n/locales/${locale}.json`, import.meta.url), 'utf8'));
    assert.equal(messages.errors.liveUpdatesInterrupted, expected);
  }
});
