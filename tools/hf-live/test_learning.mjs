import assert from "node:assert/strict";
import test from "node:test";

import { BADGE_NAME, MISSION_CATALOG } from "./web/learning-data.mjs";
import {
  PROGRESS_STORAGE_KEY, createProgressStore, findTuningEvidence, hasBadge, isMissionComplete,
  isMissionUnlocked, isTrainingRequest, observedBurstKey, readProgress, readTerminalEvidence,
} from "./web/learning-state.mjs";
import { getTrainingScenario } from "./web/training-scenarios.mjs";

const usable = getTrainingScenario("usable-link");
const weak = getTrainingScenario("weak-link");

function memory(entries = []) {
  const values = new Map(entries);
  return { getItem: key => values.get(key) ?? null, setItem: (key, value) => values.set(key, value) };
}

function summary(bandwidthHz) {
  const wide = bandwidthHz === 2300;
  return {
    bandwidthHz, scenarioId: usable.id, scenarioVersion: usable.version, outcome: "delivered",
    frameCount: wide ? 1 : 8, totalFrames: wide ? 1 : 8, airtimeS: wide ? 4.384 : 43.008, retries: 0,
  };
}

function requestFor(scenario) {
  return { id: "trial-id", from: scenario.source, to: scenario.target, kind: "chat", subject: "", body: scenario.message };
}

test("progress is versioned, counted once per mission and unlocks diagnosis only after reading", () => {
  const storage = memory();
  let changes = 0;
  const store = createProgressStore({ storage, onChange: () => changes++ });
  assert.equal(isMissionUnlocked(store.progress, "diagnose-link"), false);
  assert.equal(store.complete("diagnose-link"), false);
  assert.equal(store.complete("unknown"), false);
  assert.equal(store.complete("read-burst"), true);
  assert.equal(store.complete("read-burst"), false);
  assert.equal(changes, 1);
  assert.equal(isMissionUnlocked(readProgress(storage), "diagnose-link"), true);
  assert.equal(BADGE_NAME, "HF Operator");
  for (const id of Object.keys(MISSION_CATALOG)) store.complete(id);
  assert.equal(hasBadge(readProgress(storage)), true);
  store.reset();
  assert.equal(hasBadge(readProgress(storage)), false);
  assert.deepEqual(readProgress(storage), { version: 1, completed: {}, compare: [] });
});

test("independent stores merge completed missions and carry sanitized trials across runs", () => {
  const storage = memory();
  const first = createProgressStore({ storage });
  const second = createProgressStore({ storage });
  first.complete("read-burst");
  second.complete("encrypted-delivery");
  first.recordTrial({ ...summary(500), body: "Private body", recipient: "Some station", attempts: ["noise samples"] });
  second.recordTrial(summary(2300));
  const progress = readProgress(storage);
  assert.equal(isMissionComplete(progress, "read-burst"), true);
  assert.equal(isMissionComplete(progress, "encrypted-delivery"), true);
  assert.deepEqual(progress.compare, [summary(500), summary(2300)]);
  const persisted = storage.getItem(PROGRESS_STORAGE_KEY);
  assert.doesNotMatch(persisted, /Private body|Some station|noise samples/);
  first.recordTrial({ ...summary(500), retries: 1, airtimeS: 48.384 });
  assert.equal(readProgress(storage).compare.length, 2);
  first.reset();
  assert.deepEqual(second.load().compare, []);
});

test("invalid and inaccessible persisted data cannot break an in-session mission", () => {
  const bad = memory([[PROGRESS_STORAGE_KEY, "{not JSON}"]]);
  assert.deepEqual(readProgress(bad), { version: 1, completed: {}, compare: [] });
  bad.setItem(PROGRESS_STORAGE_KEY, JSON.stringify({
    version: 1, completed: { "diagnose-link": true, "encrypted-delivery": 1, unknown: true },
    compare: [{ ...summary(500), totalFrames: 7 }, { ...summary(2300), scenarioId: "weak-link" }],
  }));
  assert.deepEqual(readProgress(bad), { version: 1, completed: {}, compare: [] });
  const blocked = {
    getItem() { throw Error("Storage denied"); },
    setItem() { throw Error("Storage denied"); },
  };
  const store = createProgressStore({ storage: blocked });
  assert.equal(store.complete("read-burst"), true);
  assert.equal(store.canSave, false);
  assert.equal(store.recordTrial(summary(500)), true);
  assert.equal(isMissionComplete(store.load(), "read-burst"), true);
  assert.equal(store.progress.compare.length, 1);
});

test("learning evidence requires the preset request, exact run identity, and authenticated delivery", () => {
  const request = requestFor(usable);
  const data = summary(500);
  const event = {
    ...request, state: "delivered", encryption: "ECDH P-256 · AES-256-GCM", receivedFrames: 8, totalFrames: 8,
    radioEvidence: { ...data, attempts: [{ attemptId: 1, frameNumber: 1, state: "delivered" }] },
  };
  assert(isTrainingRequest(request, usable));
  assert(!isTrainingRequest({ ...request, body: "Alternate text" }, usable));
  assert(!isTrainingRequest({ ...request, to: weak.target }, usable));
  assert.equal(readTerminalEvidence(event, request, usable, 500).frameCount, 8);
  assert.equal(readTerminalEvidence({ ...event, state: "sending" }, request, usable, 500), null);
  assert.equal(readTerminalEvidence({ ...event, body: "Corrupt decoded message" }, request, usable, 500), null);
  assert.equal(readTerminalEvidence({ ...event, encryption: null }, request, usable, 500), null);
  assert.equal(readTerminalEvidence({ ...event, receivedFrames: 7 }, request, usable, 500), null);
  assert.equal(readTerminalEvidence({ ...event, id: "another trial" }, request, usable, 500), null);
  assert.equal(readTerminalEvidence(event, request, usable, 2300), null);
  assert.equal(readTerminalEvidence(event, request, { ...usable, version: usable.version + 1 }, 500), null);
  assert.equal(readTerminalEvidence({ ...event, radioEvidence: null }, request, usable, 500), null);
});

test("weak-path diagnosis needs a measured failure, a later tuning change, and terminal failure", () => {
  const failure = { attemptId: 4, state: "failed", retry: 0, frameNumber: 1, band: "80m", powerW: 1000, measuredSinrDb: -17 };
  const tuning = { ...failure, attemptId: 5, retry: 1, band: "40m" };
  assert.deepEqual(findTuningEvidence({ outcome: "failed", attempts: [failure, tuning] }), { failure, tuning });
  assert.equal(findTuningEvidence({ outcome: "delivered", attempts: [failure, tuning] }), null);
  assert.equal(findTuningEvidence({ outcome: "failed", attempts: [failure, { ...tuning, band: "80m" }] }), null);
  assert.equal(findTuningEvidence({ outcome: "failed", attempts: [failure, { ...tuning, frameNumber: 2 }] }), null);
  assert.equal(findTuningEvidence({ outcome: "failed", attempts: [{ ...failure, measuredSinrDb: null }, tuning] }), null);
});

test("signal observation keys reject unrelated or stale attempt identities", () => {
  const attempt = { id: 19, source: usable.source, target: usable.target, started_at_s: 0.2 };
  assert.equal(observedBurstKey(attempt, usable, "19"), "19/0.2");
  assert.notEqual(observedBurstKey({ ...attempt, started_at_s: 1.2 }, usable, 19), "19/0.2");
  assert.equal(observedBurstKey({ ...attempt, id: 20 }, usable, 19), null);
  assert.equal(observedBurstKey(attempt, weak, 19), null);
  assert.equal(observedBurstKey(attempt, usable, null), null);
});
