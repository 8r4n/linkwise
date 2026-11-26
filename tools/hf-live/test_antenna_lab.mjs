import assert from "node:assert/strict";
import test from "node:test";
import {
  comparisonReady, createLabProgressStore, labTrial, LAB_PROGRESS_KEY,
  readLabProgress, readTrialEvidence,
} from "./web/antenna-lab-state.mjs";
import { PROGRESS_STORAGE_KEY } from "./web/learning-state.mjs";

const fixture = {
  fixtureVersion: 1, source: "SIM-022", target: "SIM-038", band: "20m",
  powerW: 1, txBearingDeg: 350, rxBearingDeg: 125, seed: 42, payloadId: "known-frame", startUtc: "2026-11-26T05:36:00Z",
};
const match = { ...fixture, startedAtS: 0 };

function evidence(spec, attemptId, { eirpW = 4, txGain = 7, rxGain = 7, sinr = 12, state = "delivered" } = {}) {
  const request = { id: "antenna-" + attemptId, ...spec.request(fixture) };
  const value = {
    id: request.id, attemptId, state, comparisonKey: "fixture-1", match: { ...match },
    attempt: {
      id: attemptId, state, band: "20m", source: fixture.source, target: fixture.target, waveform_mode: "BW500",
      started_at_s: 0, airtime_s: 5.376, payload_bytes: 9,
      measured_snr_db: sinr + 1, measured_sinr_db: sinr,
      antenna: {
        conductedW: 1, eirpW,
        tx: { profileId: request.txPort, headingDeg: request.txAzimuth, gainDb: txGain, bearingDeg: fixture.txBearingDeg },
        rx: { profileId: request.rxPort, headingDeg: request.rxAzimuth, gainDb: rxGain, bearingDeg: fixture.rxBearingDeg },
      },
    },
  };
  return { request, value };
}

test("requests wrap azimuth and change only the endpoint under study", () => {
  const transmit = labTrial.tx.map(spec => spec.request(fixture));
  assert.deepEqual(transmit.map(value => value.txAzimuth), [350, 350, 80]);
  assert.deepEqual(transmit.map(value => value.txPort), ["dipole-hf", "yagi-20m", "yagi-20m"]);
  assert.ok(transmit.every(value => value.rxAzimuth === 125 && value.rxPort === "yagi-20m"));
  const receive = labTrial.rx.map(spec => spec.request(fixture));
  assert.deepEqual(receive.map(value => value.rxPort), ["dipole-hf", "active-loop"]);
  assert.ok(receive.every(value => value.txAzimuth === 350 && value.txPort === "yagi-20m" && value.rxAzimuth === 125));
});

test("requires all three actual matched transmit results, accepting a decoded and a failed frame", () => {
  const results = new Map();
  for (const [index, spec] of labTrial.tx.entries()) {
    const { request, value } = evidence(spec, index + 1, {
      eirpW: [3, 4, 1][index], txGain: [4, 7, -8][index], sinr: [15, 18, -11][index],
      state: index === 2 ? "failed" : "delivered",
    });
    const result = readTrialEvidence(value, fixture, request);
    assert.ok(result);
    assert.equal(result.state, value.state);
    assert.equal(result.source, fixture.source);
    assert.equal(result.attemptId, index + 1);
    results.set(spec.id, result);
    assert.equal(comparisonReady("tx", results), index === 2);
  }
  const last = results.get(labTrial.tx[2].id);
  results.set(labTrial.tx[2].id, { ...last, match: { ...last.match, payloadId: "changed" } });
  assert.equal(comparisonReady("tx", results), false);
});

test("requires constant transmit EIRP and distinct measured SINR for the receive exercise", () => {
  const results = new Map();
  for (const [index, spec] of labTrial.rx.entries()) {
    const { request, value } = evidence(spec, index + 5, {
      sinr: 13 - index * 21, rxGain: 7 - index * 20, state: index === 1 ? "failed" : "delivered",
    });
    results.set(spec.id, readTrialEvidence(value, fixture, request));
  }
  assert.equal(comparisonReady("rx", results), true);
  const other = results.get(labTrial.rx[1].id);
  results.set(labTrial.rx[1].id, { ...other, eirpDbw: other.eirpDbw + 0.1 });
  assert.equal(comparisonReady("rx", results), false);
  results.set(labTrial.rx[1].id, { ...other, sinrDb: results.get(labTrial.rx[0].id).sinrDb });
  assert.equal(comparisonReady("rx", results), false);
});

test("rejects pending, unrelated, mismatched, and incomplete worker evidence", () => {
  const { request, value } = evidence(labTrial.tx[0], 4, { state: "failed" });
  assert.ok(readTrialEvidence(value, fixture, request));
  const invalid = [
    { ...value, state: "transmitting" },
    { ...value, id: "old-command" },
    { ...value, attemptId: 5 },
    { ...value, attempt: { ...value.attempt, target: "SIM-004" } },
    { ...value, attempt: { ...value.attempt, measured_sinr_db: null } },
    { ...value, attempt: { ...value.attempt, started_at_s: 1 } },
    { ...value, match: { ...value.match, powerW: 5 } },
    { ...value, attempt: { ...value.attempt, antenna: { ...value.attempt.antenna, tx: { ...value.attempt.antenna.tx, headingDeg: 77 } } } },
  ];
  for (const event of invalid) assert.equal(readTrialEvidence(event, fixture, request), null);
});

test("stores two independent completion flags and clears only the lab", () => {
  const data = new Map([[PROGRESS_STORAGE_KEY, JSON.stringify({ version: 1, completed: { "read-burst": true } })]]);
  const storage = { getItem: key => data.get(key), setItem: (key, value) => data.set(key, value) };
  const original = data.get(PROGRESS_STORAGE_KEY);
  const first = createLabProgressStore({ storage });
  const second = createLabProgressStore({ storage });
  first.complete("tx");
  second.complete("rx");
  assert.deepEqual(readLabProgress(storage).completed, { tx: true, rx: true });
  second.reset();
  assert.deepEqual(readLabProgress(storage).completed, { tx: false, rx: false });
  assert.equal(data.get(PROGRESS_STORAGE_KEY), original);
  data.set(LAB_PROGRESS_KEY, '{"version":1,"completed":{"tx":"true","rx":true,"read-burst":true}}');
  assert.deepEqual(readLabProgress(storage).completed, { tx: false, rx: true });
  data.set(LAB_PROGRESS_KEY, "{incomplete");
  assert.deepEqual(readLabProgress(storage).completed, { tx: false, rx: false });
});
