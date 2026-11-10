/** A document-local boundary for requests, sockets and session-owned state. */
export interface BrowserSession {
  id: string;
  admin: boolean;
}

let generation = 0;
let session: BrowserSession | null = null;
let controller = new AbortController();
const listeners = new Set<(session: BrowserSession | null) => void>();

export function sessionContext() {
  return { generation, signal: controller.signal, session };
}

/** Call before changing account, server or authentication state. */
export function advanceSessionContext() {
  generation++;
  controller.abort();
  controller = new AbortController();
  session = null;
  for (const listener of listeners) listener(null);
}

export function confirmSessionContext(value: BrowserSession | null) {
  if (session?.id !== value?.id || session?.admin !== value?.admin) {
    advanceSessionContext();
  }
  session = value ? { id: value.id, admin: value.admin } : null;
  for (const listener of listeners) listener(session);
}

export function onSessionContext(
  listener: (value: BrowserSession | null) => void,
) {
  listeners.add(listener);
  listener(session);
  return () => listeners.delete(listener);
}
