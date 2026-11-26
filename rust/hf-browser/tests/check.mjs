import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const bytes = await readFile(process.argv[2]);
const { instance } = await WebAssembly.instantiate(bytes);
const wasm = instance.exports;
const rate = wasm.lw_sample_rate();
const payload = new Uint8Array([0, 255, 0x14, 0x33, 0, 0, 0x81, 0x80, 0x07]);

function pcm(kind, count) {
  const pointer = wasm.lw_buffer_ptr(kind);
  return new Float32Array(wasm.memory.buffer, pointer, count);
}

function mailbox(count) {
  const pointer = wasm.lw_payload_ptr();
  return new Uint8Array(wasm.memory.buffer, pointer, count);
}

function expectPayload(samples, expected) {
  pcm(0, samples.length).set(samples);
  assert.equal(wasm.lw_decode(samples.length), 1);
  assert.equal(wasm.lw_decoded_len(), expected.length);
  assert.deepEqual(mailbox(expected.length), expected);
}

assert.equal(rate, 48_000);
assert.equal(wasm.lw_buffer_ptr(3), 0);
assert.equal(wasm.lw_set_mode(2751), 0);
assert.equal(wasm.lw_payload_capacity(), 9);
for (const bandwidth of [500, 2300, 2750]) {
  assert.equal(wasm.lw_set_mode(bandwidth), 1);
  const capacity = bandwidth === 500 ? 9 : 86;
  assert.equal(wasm.lw_payload_capacity(), capacity);
  assert.equal(wasm.lw_encode_len(capacity + 1), 0);
  assert.equal(wasm.lw_encode_len(0), 0);

  mailbox(payload.length).set(payload);
  const started = performance.now();
  const count = wasm.lw_encode();
  assert.equal(count, bandwidth === 500 ? 258_048 : 210_432);
  const samples = pcm(0, count).slice();
  expectPayload(samples, payload);
  if (bandwidth !== 500) {
    const data = Uint8Array.from({ length: capacity }, (_, i) => i * 73 % 256);
    mailbox(capacity).set(data);
    assert.equal(wasm.lw_encode_len(capacity), count);
    expectPayload(pcm(0, count).slice(), data);
  }

  pcm(0, count).set(samples);
  const sourcePower = wasm.lw_reference_power(0, count);
  const propagated = wasm.lw_propagate(count, 17, 47, 48_000, 32, 1);
  assert.equal(propagated, count + 48);
  assert.deepEqual(pcm(0, count), samples);
  const signal = pcm(1, propagated).slice();
  const noise = pcm(2, propagated).slice();
  const measured = 10 * Math.log10(sourcePower / wasm.lw_reference_power(2, propagated));
  assert.ok(Math.abs(measured - 32) < 0.5, measured);
  expectPayload(signal.map((value, i) => value + noise[i]), payload);

  pcm(0, count).set(samples);
  assert.equal(wasm.lw_propagate(count, 999, 47, 48_000, 32, 0), propagated);
  assert.deepEqual(pcm(1, propagated), signal);
  assert.ok(pcm(2, propagated).every((value) => value === 0));
  assert.equal(wasm.lw_propagate(count, 17, 47, 48_000, -80, 1), propagated);
  const receive = pcm(0, propagated);
  const lossSignal = pcm(1, propagated);
  const lossNoise = pcm(2, propagated);
  for (let i = 0; i < propagated; i += 1) receive[i] = lossSignal[i] + lossNoise[i];
  assert.equal(wasm.lw_decode(propagated), 0);
  assert.equal(wasm.lw_decoded_len(), 0);
  console.log(`${bandwidth} Hz: ${count} samples, calibration ${measured.toFixed(2)} dB, ${Math.round(performance.now() - started)} ms`);
}

assert.equal(wasm.lw_decode(0), -1);
assert.equal(wasm.lw_decode(wasm.lw_max_samples() + 1), -1);
assert.equal(wasm.lw_propagate(100, 1, 1, -1, 20, 1), 0);
pcm(0, 1)[0] = Number.NaN;
assert.equal(wasm.lw_decode(1), -1);
assert.ok(Number.isNaN(wasm.lw_reference_power(0, 1)));
