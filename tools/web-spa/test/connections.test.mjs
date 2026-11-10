import assert from 'node:assert/strict';
import test from 'node:test';
import { WebSocketHandler } from '../../../server/composables/ws.ts';
import { advanceSessionContext, confirmSessionContext } from '../../../server/composables/session-context.ts';
import { startNotificationConnection, useNotifications, useNotificationConnection } from '../../../server/composables/notifications.ts';
import { startTaskConnection, useTask, useTaskReady } from '../../../server/composables/task.ts';
import { startSystemConnection, useSystemData, useSystemConnection } from '../../../server/composables/admin-home.ts';

test('channels follow sessions, reconnect with capped backoff and stop on auth failure', async () => {
  const originals = new Map();
  const install = (name, value) => {
    originals.set(name, Object.getOwnPropertyDescriptor(globalThis, name));
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value });
  };
  const sockets = [];
  const timers = new Map();
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
  install('location', { protocol: 'https:', host: 'example.invalid', href: 'https://example.invalid/store' });
  install('setTimeout', (callback, delay) => { const id = ++timerId; timers.set(id, { callback, delay }); return id; });
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
      assert.equal(retry(), delay);
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
  } finally {
    for (const dispose of disposers) dispose();
    advanceSessionContext();
    for (const [name, descriptor] of originals) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else delete globalThis[name];
    }
  }
});
