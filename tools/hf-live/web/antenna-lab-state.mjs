export const LAB_PROGRESS_KEY = "linkwise:antenna-lab:v1";
export const LAB_PROGRESS_EVENT = "linkwise-antenna-lab-progress";
export const LAB_PREREQUISITE = "read-burst";
export const LAB_EXERCISES = Object.freeze(["tx", "rx"]);

function localStorageArea() {
  try { return globalThis.localStorage; } catch { return undefined; }
}

function emptyProgress() {
  return { version: 1, completed: { tx: false, rx: false } };
}

export function readLabProgress(storage = localStorageArea()) {
  try {
    const raw = JSON.parse(storage?.getItem(LAB_PROGRESS_KEY) ?? "null");
    if (raw?.version === 1) {
      return {
        version: 1,
        completed: Object.fromEntries(LAB_EXERCISES.map(exercise => [exercise, raw.completed?.[exercise] === true])),
      };
    }
  } catch {
    // Malformed or blocked storage begins an empty session.
  }
  return emptyProgress();
}

export function createLabProgressStore({ storage = localStorageArea(), onChange = () => {} } = {}) {
  let progress = readLabProgress(storage);
  let canSave = Boolean(storage);

  function publish() {
    try { storage?.setItem(LAB_PROGRESS_KEY, JSON.stringify(progress)); } catch { canSave = false; }
    onChange();
  }

  return {
    get progress() { return progress; },
    get canSave() { return canSave; },
    reload() {
      if (canSave) progress = readLabProgress(storage);
    },
    complete(exercise) {
      if (!LAB_EXERCISES.includes(exercise)) return;
      const stored = readLabProgress(storage);
      progress = {
        version: 1,
        completed: Object.fromEntries(LAB_EXERCISES.map(id => [
          id, id === exercise || progress.completed[id] || stored.completed[id],
        ])),
      };
      publish();
    },
    reset() {
      progress = emptyProgress();
      publish();
    },
  };
}

const normalizeAzimuth = value => ((value % 360) + 360) % 360;

function request(fixture, txPort, rxPort, txOffset = 0) {
  return {
    txPort,
    rxPort,
    txAzimuth: normalizeAzimuth(fixture.txBearingDeg + txOffset),
    rxAzimuth: normalizeAzimuth(fixture.rxBearingDeg),
  };
}

export const labTrial = Object.freeze({
  tx: [
    { id: "tx-dipole", label: "Dipole", button: "Run dipole", request: fixture => request(fixture, "dipole-hf", "yagi-20m") },
    { id: "tx-aligned", label: "Aligned Yagi", button: "Run aligned Yagi", request: fixture => request(fixture, "yagi-20m", "yagi-20m") },
    { id: "tx-off-axis", label: "Yagi 90° off axis", button: "Run off-axis Yagi", request: fixture => request(fixture, "yagi-20m", "yagi-20m", 90) },
  ],
  rx: [
    { id: "rx-dipole", label: "Dipole", button: "Run receive dipole", request: fixture => request(fixture, "yagi-20m", "dipole-hf") },
    { id: "rx-loop", label: "Active receiving loop", button: "Run receive loop", request: fixture => request(fixture, "yagi-20m", "active-loop") },
  ],
});

const stableKeys = ["fixtureVersion", "seed", "band", "powerW", "startedAtS", "payloadId", "source", "target", "startUtc"];

function sameAngle(first, second) {
  return Number.isFinite(first) && Number.isFinite(second)
    && Math.abs(normalizeAzimuth(first - second + 180) - 180) < .05;
}

function validMount(mount, profile, heading, bearing) {
  return mount?.profileId === profile && Number.isFinite(mount.gainDb)
    && sameAngle(mount.headingDeg, heading) && sameAngle(mount.bearingDeg, bearing);
}

export function readTrialEvidence(value, fixture, request) {
  const { state, attemptId, attempt, comparisonKey, match } = value ?? {};
  if (!fixture || value?.id !== request.id || !["delivered", "failed"].includes(state)
    || !Number.isSafeInteger(attemptId) || attemptId <= 0 || attempt?.id !== attemptId
    || attempt.state !== state || attempt.band !== fixture.band || attempt.waveform_mode !== "BW500"
    || attempt.source !== fixture.source || attempt.target !== fixture.target
    || typeof comparisonKey !== "string" || !comparisonKey || !match
    || match.fixtureVersion !== fixture.fixtureVersion || match.band !== fixture.band
    || match.source !== fixture.source || match.target !== fixture.target
    || match.powerW !== fixture.powerW || !Number.isSafeInteger(match.seed) || match.seed !== fixture.seed
    || match.startedAtS !== 0 || attempt.started_at_s !== match.startedAtS
    || !Number.isFinite(attempt.airtime_s) || attempt.airtime_s <= 0
    || !Number.isSafeInteger(attempt.payload_bytes) || attempt.payload_bytes <= 0
    || match.startUtc !== fixture.startUtc
    || typeof match.payloadId !== "string" || !match.payloadId || match.payloadId !== fixture.payloadId) {
    return null;
  }

  const { antenna, measured_snr_db: snrDb, measured_sinr_db: sinrDb } = attempt;
  if (antenna?.conductedW !== fixture.powerW || !Number.isFinite(antenna.eirpW) || antenna.eirpW <= 0
    || !validMount(antenna.tx, request.txPort, request.txAzimuth, fixture.txBearingDeg)
    || !validMount(antenna.rx, request.rxPort, request.rxAzimuth, fixture.rxBearingDeg)
    || !Number.isFinite(snrDb) || !Number.isFinite(sinrDb) || sinrDb > snrDb + .01) {
    return null;
  }

  return {
    comparisonKey, match, state, attemptId, source: attempt.source, tx: antenna.tx, rx: antenna.rx,
    airtime: attempt.airtime_s, payloadBytes: attempt.payload_bytes,
    powerW: antenna.conductedW, txAzimuth: antenna.tx.headingDeg, rxAzimuth: antenna.rx.headingDeg,
    eirpDbw: 10 * Math.log10(antenna.eirpW), rxLabel: antenna.rx.label ?? antenna.rx.profileId, snrDb, sinrDb,
  };
}

function sameMount(first, second) {
  return first.profileId === second.profileId && sameAngle(first.headingDeg, second.headingDeg)
    && first.gainDb === second.gainDb;
}

export function comparisonReady(exercise, results) {
  const candidates = labTrial[exercise]?.map(spec => results.get(spec.id));
  if (!candidates?.every(Boolean)) return false;
  const [first, second, third] = candidates;
  if (new Set(candidates.map(result => result.attemptId)).size !== candidates.length
    || !candidates.every(result => result.comparisonKey === first.comparisonKey
      && result.airtime === first.airtime && result.payloadBytes === first.payloadBytes
      && stableKeys.every(key => result.match[key] === first.match[key]))) {
    return false;
  }
  if (exercise === "tx") {
    return candidates.every(result => sameMount(first.rx, result.rx))
      && second.eirpDbw > first.eirpDbw && first.eirpDbw > third.eirpDbw;
  }
  return sameMount(first.tx, second.tx) && first.eirpDbw === second.eirpDbw
    && first.rx.profileId !== second.rx.profileId && sameAngle(first.rx.headingDeg, second.rx.headingDeg)
    && Math.abs(first.sinrDb - second.sinrDb) >= .1;
}
