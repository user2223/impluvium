import taskHandler from "~/server/internal/tasks";
import aclManager from "~/server/internal/acls";
import type { MinimumRequestObject } from "~/server/h3";

// TODO add web socket sessions for horizontal scaling
// ID to admin
const socketHeaders = new Map<string, MinimumRequestObject>();

function detach(id: string) {
  socketHeaders.delete(id);
  taskHandler.disconnectAll(id, false);
}

export default defineWebSocketHandler({
  async open(peer) {
    const request = peer.request;
    if (!request) {
      detach(peer.id);
      peer.send("unauthenticated");
      return;
    }

    const headers = {
      headers: request.headers ?? new Headers(),
    };
    socketHeaders.set(peer.id, headers);
    if (!(await aclManager.isAuthenticated(headers))) {
      detach(peer.id);
      peer.send("unauthenticated");
      return;
    }

    if (socketHeaders.get(peer.id) !== headers) return;
    peer.send(`connect`);
  },
  async message(peer, message) {
    if (!peer.id) return;
    const headers = socketHeaders.get(peer.id);
    // Idle subscriptions are revalidated only on the next incoming message.
    if (headers === undefined || !(await aclManager.isAuthenticated(headers))) {
      detach(peer.id);
      peer.send("unauthenticated");
      return;
    }
    if (socketHeaders.get(peer.id) !== headers) return;
    const text = message.text();
    if (text.startsWith("connect/")) {
      const id = text.substring("connect/".length);
      await taskHandler.connect(
        peer.id,
        id,
        peer,
        headers,
        () => socketHeaders.get(peer.id) === headers,
      );
      return;
    }
  },
  close(peer, _details) {
    if (!peer.id) return;
    detach(peer.id);
  },
});
