import assert from "node:assert/strict";
import test from "node:test";
import { createStationMessaging } from "./web/station-messaging.js";

const request = { id: "test-mail", from: "SIM-001", to: "SIM-031", kind: "email",
  subject: "Mail", body: "Météo / terrain" };

test("encrypted mail reassembles from HF fragments and opens only at the recipient", async () => {
  for (const capacity of [9, 86]) {
    const events = [];
    const packets = [];
    const messaging = createStationMessaging({
      frameBytes: capacity,
      emit: event => events.push(event),
      sendFrame: async (from, to, packet) => {
        assert.equal(from, request.from);
        assert.equal(to, request.to);
        assert.ok(packet.length <= capacity);
        packets.push(packet);
        return packet.slice();
      },
    });
    await messaging.send(request);
    const result = events.at(-1);
    assert.equal(result.state, "delivered");
    assert.equal(result.body, request.body);
    assert.equal(result.subject, request.subject);
    assert.equal(result.totalFrames, packets.length);
    assert.equal(capacity === 9, packets.length > 1);
    assert.ok(events.slice(0, -1).every(event => !Object.hasOwn(event, "body")));
    assert.ok(!Buffer.concat(packets.map(packet => Buffer.from(packet))).includes(Buffer.from(request.body)));
  }
});

test("corruption and lost HF frames keep the message out of the inbox", async () => {
  for (const fault of ["corrupted", "lost"]) {
    const events = [];
    const messaging = createStationMessaging({
      frameBytes: 9,
      emit: event => events.push(event),
      sendFrame: async (_, __, packet) => {
        if (fault === "lost") return null;
        const damaged = packet.slice();
        damaged[damaged.length - 1] ^= 1;
        return damaged;
      },
    });
    await messaging.send({ ...request, id: fault, kind: "chat", subject: "" });
    assert.equal(events.at(-1).state, "failed");
    assert.ok(events.every(event => !Object.hasOwn(event, "body")));
  }
});
