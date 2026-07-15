"use strict";

// Persistent connection to webspec-index native-messaging host.
// Re-connects on disconnect so the extension survives binary restarts.

const HOST = "webspec_index";

let port = null;
let pendingRequests = new Map(); // id → {resolve, reject, timer}
let nextId = 1;

function connect() {
  port = browser.runtime.connectNative(HOST);

  port.onMessage.addListener((msg) => {
    const pending = pendingRequests.get(msg.id);
    if (!pending) return;
    pendingRequests.delete(msg.id);
    clearTimeout(pending.timer);
    if (msg.ok) {
      pending.resolve(msg);
    } else {
      pending.reject(new Error(msg.error || "unknown error"));
    }
  });

  port.onDisconnect.addListener(() => {
    // Reject all outstanding requests
    for (const [id, pending] of pendingRequests) {
      clearTimeout(pending.timer);
      pending.reject(new Error("native messaging host disconnected"));
    }
    pendingRequests.clear();
    port = null;
  });
}

function send(payload) {
  return new Promise((resolve, reject) => {
    if (!port) {
      try {
        connect();
      } catch (e) {
        reject(new Error("failed to connect to webspec-index: " + e.message));
        return;
      }
    }

    const id = nextId++;
    const timer = setTimeout(() => {
      pendingRequests.delete(id);
      reject(new Error("timeout waiting for webspec-index"));
    }, 15000);

    pendingRequests.set(id, { resolve, reject, timer });
    port.postMessage({ ...payload, id });
  });
}

browser.runtime.onMessage.addListener((msg, _sender) => {
  if (msg.type === "query") {
    return send({ type: "query", url: msg.url });
  }
  if (msg.type === "search") {
    return send({ type: "search", spec: msg.spec, query: msg.query });
  }
  if (msg.type === "list") {
    return send({ type: "list", spec: msg.spec });
  }
});

connect();
