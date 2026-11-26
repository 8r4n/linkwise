const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true });
const HEADER = 4;
const LIMIT = 4;

export function textBytes(value) {
  return encoder.encode(value).length;
}

export function messageLimit(frameBytes) {
  return frameBytes <= 9 ? 64 : 240;
}

export function createStationMessaging({ emit, sendFrame, frameBytes }) {
  const stations = new Map();
  const active = new Map();
  let number = 0;
  let closed = false;

  function station(id) {
    if (!stations.has(id)) {
      const keys = crypto.subtle.generateKey({ name: "ECDH", namedCurve: "P-256" }, false, ["deriveKey"]);
      stations.set(id, keys);
    }
    return stations.get(id);
  }

  async function session(receiver, sender, usages) {
    const [own, peer] = await Promise.all([station(receiver), station(sender)]);
    return crypto.subtle.deriveKey({ name: "ECDH", public: peer.publicKey }, own.privateKey,
      { name: "AES-GCM", length: 256 }, false, usages);
  }

  function status(request, state, fields = {}) {
    emit({ id: request.id, from: request.from, to: request.to, kind: request.kind, state, ...fields });
  }

  async function send(value) {
    const request = value && typeof value === "object" ? value : {};
    const { id, from, to, kind, body } = request;
    const subject = kind === "email" ? request.subject : "";
    if (closed) return;
    if (typeof id !== "string" || id.length > 80 || !/^SIM-\d{3}$/.test(from) || !/^SIM-\d{3}$/.test(to)
      || from === to || !["chat", "email"].includes(kind) || typeof body !== "string" || !body.trim()
      || typeof subject !== "string" || (kind === "email" && !subject.trim())
      || textBytes(subject) > 48 || textBytes(subject) + textBytes(body) > messageLimit(frameBytes)) {
      status(request, "failed", { error: "Check the destination and the message length." });
      return;
    }
    if (active.has(id)) return;
    if (active.size >= LIMIT || [...active.values()].some(item => item.from === from)) {
      status(request, "failed", { error: "This station is already sending or the message queue is full." });
      return;
    }
    if (!globalThis.crypto?.subtle) {
      status(request, "failed", { error: "Encrypted messaging requires HTTPS or a localhost preview." });
      return;
    }
    active.set(id, { id, from, to, kind });
    const ticket = (++number) & 0xffff;
    try {
      status(request, "encrypting");
      const aad = encoder.encode(JSON.stringify(["Linkwise browser HF v1", ticket, from, to, kind]));
      const title = encoder.encode(subject);
      const text = encoder.encode(body);
      const plain = new Uint8Array(1 + title.length + text.length);
      plain[0] = title.length;
      plain.set(title, 1);
      plain.set(text, 1 + title.length);
      const iv = crypto.getRandomValues(new Uint8Array(12));
      const key = await session(from, to, ["encrypt"]);
      const sealed = new Uint8Array(await crypto.subtle.encrypt({ name: "AES-GCM", iv, additionalData: aad }, key, plain));
      const wire = new Uint8Array(iv.length + sealed.length);
      wire.set(iv);
      wire.set(sealed, iv.length);
      const capacity = frameBytes - HEADER;
      const total = Math.ceil(wire.length / capacity);
      const received = new Uint8Array(wire.length);
      let receivedLength = 0;
      for (let index = 0; index < total; index++) {
        if (closed) return;
        const slice = wire.subarray(index * capacity, (index + 1) * capacity);
        const packet = new Uint8Array(HEADER + slice.length);
        const header = new DataView(packet.buffer);
        header.setUint16(0, ticket);
        packet[2] = index | (index === total - 1 ? 0x80 : 0);
        packet[3] = slice.length;
        packet.set(slice, HEADER);
        status(request, "sending", { receivedFrames: index, totalFrames: total });
        const output = await sendFrame(from, to, packet, { id, frameNumber: index + 1, totalFrames: total });
        if (closed) return;
        if (!output) {
          status(request, "failed", { receivedFrames: index, totalFrames: total,
            error: "The HF link exhausted retries for a frame." });
          return;
        }
        if (output.length < HEADER || output[0] !== packet[0] || output[1] !== packet[1]
          || output[2] !== packet[2] || output[3] !== slice.length || output.length !== packet.length) {
          status(request, "failed", { receivedFrames: index, totalFrames: total,
            error: "The recipient could not reassemble the message." });
          return;
        }
        received.set(output.subarray(HEADER), receivedLength);
        receivedLength += output[3];
      }
      const openKey = await session(to, from, ["decrypt"]);
      const opened = new Uint8Array(await crypto.subtle.decrypt({
        name: "AES-GCM", iv: received.subarray(0, 12), additionalData: aad,
      }, openKey, received.subarray(12)));
      if (closed) return;
      const endOfSubject = opened[0] + 1;
      status(request, "delivered", {
        receivedFrames: total, totalFrames: total, subject: decoder.decode(opened.subarray(1, endOfSubject)),
        body: decoder.decode(opened.subarray(endOfSubject)), encryption: "ECDH P-256 · AES-256-GCM",
      });
    } catch {
      if (!closed) status(request, "failed", { error: "The recipient could not authenticate and open the message." });
    } finally {
      active.delete(id);
    }
  }

  return {
    send,
    close() {
      closed = true;
      for (const request of active.values()) status(request, "failed", { error: "The simulation restarted." });
      stations.clear();
    },
  };
}
