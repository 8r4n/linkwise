import { MISSION_CATALOG } from "./learning-data.mjs";

export const PROGRESS_STORAGE_KEY = "linkwise:skill-tree:v1";
export const PROGRESS_EVENT = "linkwise-progress";
export const COMPARE_BANDWIDTHS = Object.freeze([500, 2300]);

function localStorageArea() {
  try { return globalThis.localStorage; } catch { return undefined; }
}

function freshProgress() {
  return { version: 1, completed: {}, compare: [] };
}

export function isMissionComplete(progress, id) {
  return Object.hasOwn(MISSION_CATALOG, id) && progress?.completed?.[id] === true;
}

export function isMissionUnlocked(progress, id) {
  const mission = Object.hasOwn(MISSION_CATALOG, id) ? MISSION_CATALOG[id] : null;
  return Boolean(mission && (!mission.requires || isMissionComplete(progress, mission.requires)));
}

export function hasBadge(progress) {
  return Object.keys(MISSION_CATALOG).every(id => isMissionComplete(progress, id));
}

function trialSummary(value) {
  if (!value || !COMPARE_BANDWIDTHS.includes(value.bandwidthHz)
      || value.scenarioId !== MISSION_CATALOG["compare-modes"].scenario
      || !Number.isSafeInteger(value.scenarioVersion) || value.scenarioVersion < 1
      || !Number.isSafeInteger(value.frameCount) || value.frameCount < 1
      || !Number.isSafeInteger(value.totalFrames) || value.totalFrames < value.frameCount
      || !Number.isFinite(value.airtimeS) || value.airtimeS <= 0
      || !Number.isSafeInteger(value.retries) || value.retries < 0
      || !["delivered", "failed"].includes(value.outcome)) return null;
  const { bandwidthHz, scenarioId, scenarioVersion, frameCount, totalFrames, airtimeS, retries, outcome } = value;
  return { bandwidthHz, scenarioId, scenarioVersion, frameCount, totalFrames, airtimeS, retries, outcome };
}

export function readProgress(storage = localStorageArea()) {
  const progress = freshProgress();
  try {
    const raw = JSON.parse(storage?.getItem(PROGRESS_STORAGE_KEY) ?? "null");
    if (raw?.version !== progress.version) return progress;
    for (const id of Object.keys(MISSION_CATALOG)) {
      if (isMissionComplete(raw, id) && isMissionUnlocked(progress, id)) progress.completed[id] = true;
    }
    for (const value of Array.isArray(raw.compare) ? raw.compare.slice(0, COMPARE_BANDWIDTHS.length) : []) {
      const trial = trialSummary(value);
      if (trial && !progress.compare.some(previous => previous.bandwidthHz === trial.bandwidthHz)) progress.compare.push(trial);
    }
  } catch {
    return progress;
  }
  return progress;
}

export function createProgressStore({ storage = localStorageArea(), onChange = () => {} } = {}) {
  let current = readProgress(storage);
  let canSave = Boolean(storage);

  function load() {
    if (canSave) current = readProgress(storage);
    return current;
  }

  function publish() {
    try { storage?.setItem(PROGRESS_STORAGE_KEY, JSON.stringify(current)); } catch { canSave = false; }
    onChange(current);
  }

  return {
    get progress() { return current; },
    get canSave() { return canSave; },
    load,
    complete(id) {
      load();
      if (!isMissionUnlocked(current, id) || isMissionComplete(current, id)) return false;
      current.completed[id] = true;
      publish();
      return true;
    },
    recordTrial(candidate) {
      const trial = trialSummary(candidate);
      if (!trial) return false;
      load();
      current.compare = current.compare.filter(value =>
        value.scenarioVersion === trial.scenarioVersion && value.bandwidthHz !== trial.bandwidthHz);
      current.compare.push(trial);
      publish();
      return true;
    },
    reset() {
      current = freshProgress();
      publish();
    },
  };
}

export function isTrainingRequest(request, scenario) {
  return Boolean(request && scenario && request.kind === "chat" && request.from === scenario.source
    && request.to === scenario.target && request.body === scenario.message);
}

export function readTerminalEvidence(event, request, scenario, bandwidth) {
  if (!request || !scenario || !isTrainingRequest(request, scenario)
      || event?.id !== request.id || event.from !== request.from || event.to !== request.to || event.kind !== request.kind
      || !["delivered", "failed"].includes(event.state)) return null;
  if (event.state === "delivered"
      && (event.body !== scenario.message || event.encryption !== "ECDH P-256 · AES-256-GCM"
        || event.receivedFrames !== event.totalFrames)) return null;
  const evidence = event.radioEvidence;
  if (evidence?.scenarioId !== scenario.id || evidence.scenarioVersion !== scenario.version
      || evidence.bandwidthHz !== bandwidth || evidence.outcome !== event.state
      || !Number.isSafeInteger(evidence.frameCount) || evidence.frameCount < 1
      || !Number.isSafeInteger(event.totalFrames) || event.totalFrames < evidence.frameCount
      || !Number.isFinite(evidence.airtimeS) || evidence.airtimeS <= 0
      || !Number.isSafeInteger(evidence.retries) || evidence.retries < 0
      || !Array.isArray(evidence.attempts) || !evidence.attempts.length) return null;
  return { ...evidence, totalFrames: event.totalFrames };
}

export function findTuningEvidence(evidence) {
  if (evidence?.outcome !== "failed") return null;
  const attempts = evidence.attempts;
  for (const failure of attempts) {
    if (failure.state !== "failed" || !Number.isFinite(failure.measuredSinrDb)
        || !Number.isFinite(failure.powerW) || typeof failure.band !== "string") continue;
    const tuning = attempts.find(later => Number(later.attemptId) > Number(failure.attemptId)
      && later.frameNumber === failure.frameNumber && later.retry > failure.retry
      && Number.isFinite(later.powerW) && typeof later.band === "string"
      && (later.band !== failure.band || later.powerW !== failure.powerW));
    if (tuning) return { failure, tuning };
  }
  return null;
}

export function observedBurstKey(attempt, scenario, expectedId) {
  if (!attempt || !scenario || expectedId == null || String(attempt.id) !== String(expectedId)
      || attempt.source !== scenario.source || attempt.target !== scenario.target
      || !Number.isFinite(attempt.started_at_s)) return null;
  return String(attempt.id) + "/" + String(attempt.started_at_s);
}
