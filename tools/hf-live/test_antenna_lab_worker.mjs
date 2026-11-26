import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { realpath } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { Worker } from "node:worker_threads";

const staging = pathToFileURL(await realpath(process.argv[2]) + "/").href;
const shim = `
  import { parentPort, workerData } from "node:worker_threads";
  import { readFile } from "node:fs/promises";
  globalThis.self = {
    addEventListener(_type, callback) { parentPort.on("message", data => callback({ data })); },
    postMessage(data) { parentPort.postMessage(data); },
  };
  globalThis.fetch = async url => {
    const href = String(url);
    if (!href.startsWith(workerData)) {
      parentPort.postMessage({type:"error", message:"Unexpected HTTP request"});
      throw Error("This test blocks network requests.");
    }
    try { return new Response(await readFile(new URL(href))); }
    catch { return new Response("", {status:404}); }
  };
  await import(new URL("browser-worker.js", workerData));
`;
const worker = new Worker(shim, { eval: true, workerData: staging });
let nextCapture = 0;
let nextTrial = 0;
const failed = new Promise((_, reject) => {
  worker.on("error", reject);
  worker.on("message", message => {
    if (message.type === "error") reject(Error(message.message));
  });
});
let receiver;
worker.on("message", message => receiver?.(message));

function exchange(request, decide) {
  const result = new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(Error("Antenna worker exchange timed out: " + request.type)), 30000).unref();
    receiver = message => {
      try {
        const value = decide(message);
        if (value === undefined) return;
        clearTimeout(timer);
        receiver = null;
        resolve(value);
      } catch (error) {
        clearTimeout(timer);
        receiver = null;
        reject(error);
      }
    };
    worker.postMessage(request);
  });
  return Promise.race([result, failed]);
}

const digest = values => createHash("sha256").update(Buffer.from(values.buffer)).digest("hex");
try {
  const snapshot = await exchange(
    { type: "start", scenarioId: "antenna-lab", seed: 914, bandwidth: 500 },
    message => message.type === "snapshot" ? message.value : undefined,
  );
  const { source, target, band, powerW, fixtureVersion, txBearingDeg, rxBearingDeg } = snapshot.antennaLab;
  assert.equal(snapshot.antennas.enabled, true);
  assert.equal(snapshot.forecast.rows, 3);
  assert.equal(snapshot.weather.origin, "archive");
  assert.equal(fixtureVersion, 1);
  assert.equal(band, "20m");
  assert.equal(powerW, 1);
  assert.notEqual((txBearingDeg + 180) % 360, rxBearingDeg);

  function trial(txPort, rxPort, txOffset = 0) {
    const id = "test-" + ++nextTrial;
    const request = { id, txPort, rxPort, txAzimuth: (txBearingDeg + txOffset) % 360, rxAzimuth: rxBearingDeg };
    let settled = false;
    return exchange({ type: "antenna-trial", request }, message => {
      if (message.type !== "antenna-trial" || message.value.id !== id) return;
      const value = message.value;
      if (value.state === "rotating") settled = false;
      if (value.state === "transmitting") settled = true;
      if (value.state === "error") throw Error(value.error);
      if (!["failed", "delivered"].includes(value.state)) return;
      assert(settled);
      assert.equal(value.attempt.source, source);
      assert.equal(value.attempt.target, target);
      assert.equal(value.attempt.waveform_mode, "BW500");
      assert.equal(value.attempt.band, band);
      assert.equal(value.attempt.antenna.tx.profileId, txPort);
      assert.equal(value.attempt.antenna.rx.profileId, rxPort);
      assert.equal(value.attempt.antenna.conductedW, powerW);
      assert.equal(value.attempt.started_at_s, 0);
      assert(equalHeading(value.attempt.antenna.tx.headingDeg, request.txAzimuth));
      assert(equalHeading(value.attempt.antenna.rx.headingDeg, request.rxAzimuth));
      assert(Number.isFinite(value.attempt.measured_snr_db));
      return value;
    });
  }
  function equalHeading(a, b) { return Math.abs(a - b) < 1e-6; }
  function capture(attempt, stage) {
    const id = ++nextCapture;
    return exchange({ type: "capture", id, station: source, stage, attempt }, message => {
      if (message.type !== "capture" || message.id !== id) return;
      assert.equal(message.status, 200);
      assert.equal(message.value.attempt.id, attempt);
      return message.value;
    });
  }

  const dipole = await trial("dipole-hf", "yagi-20m");
  const initialRx = await capture(dipole.attemptId, "rx");
  const initialTx = await capture(dipole.attemptId, "tx");
  const aligned = await trial("yagi-20m", "yagi-20m");
  const offAxis = await trial("yagi-20m", "yagi-20m", 90);
  const receiveDipole = await trial("yagi-20m", "dipole-hf");
  const receiveLoop = await trial("yagi-20m", "active-loop");
  for (const result of [dipole, aligned, offAxis, receiveDipole, receiveLoop]) {
    assert.equal(result.comparisonKey, dipole.comparisonKey);
    assert.deepEqual(result.match, dipole.match);
  }
  const budget = result => result.attempt.antenna;
  assert(budget(aligned).eirpW > budget(dipole).eirpW);
  assert(budget(dipole).eirpW > budget(offAxis).eirpW);
  assert(Math.abs(aligned.attempt.measured_sinr_db - offAxis.attempt.measured_sinr_db - 20) < .05);
  assert.equal(aligned.state, "delivered");
  assert.equal(offAxis.state, "failed");
  assert.equal(budget(receiveDipole).eirpW, budget(receiveLoop).eirpW);
  assert.notEqual(receiveDipole.attempt.measured_sinr_db, receiveLoop.attempt.measured_sinr_db);

  const original = digest(initialRx.waveform_f32);
  // More trials than the working PCM cache can hold, but fewer than the
  // retained descriptor window, force an old RX reconstruction.
  for (let i = 0; i < 8; i++) await trial("yagi-20m", "active-loop");
  const reconstructed = await capture(dipole.attemptId, "rx");
  const repeatTx = await capture(receiveLoop.attemptId, "tx");
  assert.equal(digest(reconstructed.waveform_f32), original);
  assert.deepEqual(reconstructed.attempt.antenna, dipole.attempt.antenna);
  assert.equal(digest(repeatTx.waveform_f32), digest(initialTx.waveform_f32));

  for (const [name, result] of Object.entries({dipole, aligned, offAxis, receiveDipole, receiveLoop})) {
    console.log(name, result.state, result.attempt.antenna.eirpW, "W EIRP", result.attempt.measured_sinr_db, "dB SINR");
  }
} finally {
  await worker.terminate();
}
