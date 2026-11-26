import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile, realpath } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { Worker } from "node:worker_threads";
import { getTrainingScenario, TRAINING_ARCHIVE } from "./web/training-scenarios.mjs";

if (!process.argv[2]) throw Error("Pass the staged hf-simulation directory.");
const staging = new URL(pathToFileURL(await realpath(resolve(process.argv[2]))).href + "/");
for (const [file, digest] of Object.entries(TRAINING_ARCHIVE)) {
  const bytes = await readFile(new URL(`training-data/v1/${file}`, staging));
  assert.equal(createHash("sha256").update(bytes).digest("hex"), digest, file);
}

const shim = `
  import { parentPort, workerData } from "node:worker_threads";
  import { readFile } from "node:fs/promises";
  globalThis.self = {
    addEventListener(_type, callback) { parentPort.on("message", data => callback({data})); },
    postMessage(data) { parentPort.postMessage(data); },
  };
  globalThis.fetch = async url => {
    const href = String(url);
    if (!href.startsWith(workerData.root)) {
      parentPort.postMessage({type:"network-denied", url:href});
      throw Error("This scenario test blocks external data sources.");
    }
    try { return new Response(await readFile(new URL(href))); }
    catch { return new Response("", { status:404 }); }
  };
  await import(new URL("browser-worker.js", workerData.root));
`;

async function trial(id, bandwidthHz, holdBeforeSend = 0) {
  const scenario = getTrainingScenario(id);
  const attempts = new Map();
  const outcome = await new Promise((fulfill, reject) => {
    const worker = new Worker(shim, { eval: true, workerData: { root: staging.href } });
    let started = false;
    let terminal = null;
    const timeout = setTimeout(() => finish(reject, Error(`${id} BW${bandwidthHz} worker timed out`)), 30000);
    function finish(callback, value) {
      clearTimeout(timeout);
      void worker.terminate();
      callback(value);
    }
    worker.on("error", error => finish(reject, error));
    worker.on("message", message => {
      try {
        if (message.type === "error" || message.type === "network-denied") {
          throw Error(message.message ?? `Unexpected network request ${message.url}`);
        }
        if (message.type === "snapshot" && !started) {
          const value = message.value;
          assert.deepEqual(value.scenario, scenario);
          assert.equal(value.seed, 42);
          assert.equal(value.elapsed_s, 0);
          assert.equal(value.forecast.rows, 6);
          assert.equal(value.weather.origin, "archive");
          assert.equal(value.weather.glotec.observed_at.slice(0, 16), "2026-11-26T05:05");
          started = true;
          setTimeout(() => worker.postMessage({
            type: "send-message",
            request: { id: "test", from: scenario.source, to: scenario.target, kind: "chat", body: scenario.message },
          }), holdBeforeSend);
        } else if (message.type === "attempt") {
          const attempt = message.value.attempt;
          if (!Number.isFinite(attempt.power_w)) throw Error("Unexpected background RF in the lesson.");
          attempts.set(attempt.id, attempt);
        } else if (message.type === "station-message" && ["delivered", "failed"].includes(message.value.state)) {
          terminal = message.value;
          const [first] = terminal.radioEvidence.attempts;
          assert.ok(first, "missing real transmission evidence");
          worker.postMessage({ type: "capture", id: 1, station: scenario.source, stage: "rx", attempt: first.attemptId });
        } else if (message.type === "capture" && terminal) {
          assert.equal(message.status, 200);
          assert.equal(message.value.stage, "rx");
          assert.equal(message.value.attempt.id, terminal.radioEvidence.attempts[0].attemptId);
          assert.ok(message.value.waveform_f32.length > 0);
          finish(fulfill, terminal);
        }
      } catch (error) {
        finish(reject, error);
      }
    });
    worker.postMessage({ type: "start", scenarioId: id, bandwidth: bandwidthHz, seed: 12345 });
  });
  const evidence = outcome.radioEvidence;
  assert.equal(evidence.scenarioVersion, 1);
  assert.equal(evidence.scenarioId, id);
  assert.equal(evidence.bandwidthHz, bandwidthHz);
  assert.equal(attempts.values().next().value.started_at_s, 0);
  for (const row of evidence.attempts) {
    const attempt = attempts.get(row.attemptId);
    assert.equal(row.airtimeS, attempt.airtime_s);
    assert.equal(row.state, attempt.state);
    assert.equal(row.band, attempt.band);
    assert.equal(row.measuredSinrDb, attempt.measured_sinr_db);
  }
  assert.ok(Math.abs(evidence.airtimeS - evidence.attempts.reduce((sum, row) => sum + row.airtimeS, 0)) < 1e-10);
  return outcome;
}

const [narrow, wide, weak, weakWide] = await Promise.all([
  trial("usable-link", 500),
  trial("usable-link", 2300),
  trial("weak-link", 500),
  trial("weak-link", 2300),
]);
for (const result of [narrow, wide]) {
  const evidence = result.radioEvidence;
  assert.equal(result.state, "delivered");
  assert.equal(evidence.outcome, "delivered");
  assert.equal(evidence.frameCount, result.totalFrames);
  assert.equal(result.receivedFrames, result.totalFrames);
  assert.equal(result.body, getTrainingScenario("usable-link").message);
}
assert.ok(narrow.radioEvidence.frameCount > wide.radioEvidence.frameCount);
assert.ok(narrow.radioEvidence.airtimeS > wide.radioEvidence.airtimeS);
for (const result of [weak, weakWide]) {
  assert.equal(result.state, "failed");
  assert.equal(result.radioEvidence.outcome, "failed");
  assert.equal(result.radioEvidence.frameCount, 1);
  assert.ok(result.radioEvidence.retries > 0);
  const [first, ...retries] = result.radioEvidence.attempts;
  assert.equal(first.state, "failed");
  assert.ok(retries.some(row => row.band !== first.band || row.powerW !== first.powerW));
}

const replay = await trial("usable-link", 500, 400);
function signature(value) {
  const evidence = value.radioEvidence;
  return {
    frames: evidence.frameCount, airtime: evidence.airtimeS, retries: evidence.retries, outcome: evidence.outcome,
    tuning: evidence.attempts.map(({ frameNumber, band, powerW, state }) => [frameNumber, band, powerW, state]),
  };
}
assert.deepEqual(signature(narrow), signature(replay));
for (const outcome of [narrow, wide, weak, weakWide]) {
  const evidence = outcome.radioEvidence;
  console.log(`${evidence.scenarioId} BW${evidence.bandwidthHz}: ${JSON.stringify(signature(outcome))}`);
}
