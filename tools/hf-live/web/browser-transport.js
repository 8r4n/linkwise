export function createBrowserTransport(receive, bandwidth) {
  const worker = new Worker(new URL("./browser-worker.js", import.meta.url), { type: "module" });
  const pending = new Map();
  let nextId = 0;
  let failure = null;
  let closing = false;
  let onClose = null;

  function fail(message) {
    failure = new Error(message);
    for (const request of pending.values()) request.reject(failure);
    pending.clear();
    worker.terminate();
    if (!closing) receive("status", { running: false, error: message });
  }

  worker.addEventListener("error", event => fail(event.message || "The browser simulation worker failed."));
  worker.addEventListener("messageerror", () => fail("The browser could not read the simulation worker."));
  worker.addEventListener("message", ({ data }) => {
    if (data.type === "closed") {
      onClose?.();
    } else if (closing && data.type !== "station-message") {
      return;
    } else if (data.type === "capture") {
      const request = pending.get(data.id);
      if (!request) return;
      const { status, value } = data;
      request.resolve({ status, ok: status === 200, json: async () => value });
    } else if (data.type === "error") {
      fail(data.message);
    } else {
      receive(data.type, data.value);
    }
  });

  const requested = new URL(location.href).searchParams.get("seed");
  const seed = requested !== null && /^\d{1,10}$/.test(requested) && Number(requested) <= 0xffffffff
    ? Number(requested) : crypto.getRandomValues(new Uint32Array(1))[0];
  worker.postMessage({ type: "start", seed, bandwidth });

  return {
    requestCapture(station, { signal, stage, attempt, after }) {
      if (failure) return Promise.reject(failure);
      if (signal.aborted) return Promise.reject(signal.reason);
      const id = ++nextId;
      return new Promise((resolve, reject) => {
        const abort = () => {
          pending.delete(id);
          reject(signal.reason);
        };
        const finish = callback => result => {
          pending.delete(id);
          signal.removeEventListener("abort", abort);
          callback(result);
        };
        pending.set(id, { resolve: finish(resolve), reject: finish(reject) });
        signal.addEventListener("abort", abort, { once: true });
        worker.postMessage({ type: "capture", id, station, stage, attempt, after });
      });
    },
    setRunning(running) {
      if (!failure) worker.postMessage({ type: "run", running });
    },
    sendMessage(request) {
      if (closing || failure) throw Error("The browser simulation is unavailable.");
      worker.postMessage({ type: "send-message", request });
    },
    close() {
      closing = true;
      if (failure) return Promise.resolve();
      return new Promise(resolve => {
        const timeout = setTimeout(() => onClose(), 1500);
        onClose = () => {
          clearTimeout(timeout);
          onClose = null;
          fail("The simulation stopped.");
          resolve();
        };
        worker.postMessage({ type: "close" });
      });
    },
  };
}
