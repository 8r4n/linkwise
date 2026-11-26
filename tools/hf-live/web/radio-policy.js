// Simulated listen-before-talk and link-budget tuning for user traffic.
export const POWER_LEVELS_W = Object.freeze([100, 250, 500, 1000]);

const DEFAULT_TARGET_DB = 6;
const LISTEN_WINDOW_S = 3;
const MIN_WAIT_S = 0.3;
const DEFAULT_WAIT_S = 1;
const HISTORY_SECONDS = 300;
const clamp = (value, low, high) => Math.max(low, Math.min(high, value));
const round = (value) => Math.round(value * 100) / 100;
const bandLabel = (band) => band.replace(/m$/, " m");

function historyCorrection(history, modeledSnr, elapsedS) {
  if (!history) return 0;
  const { attempts, deliveries, lastAtS, lastPowerGainDb, lastSinrDb, lastDelivered } = history;
  const age = Number.isFinite(lastAtS) ? Math.max(0, elapsedS - lastAtS) : 0;
  const weight = Math.max(0, 1 - age / HISTORY_SECONDS);
  const delivery = Number.isInteger(attempts) && attempts > 0 && Number.isInteger(deliveries)
    ? 2.5 * Math.min(1, attempts / 3) * (2 * clamp(deliveries / attempts, 0, 1) - 1)
    : 0;
  // Compare at a common transmit power. A high-power measurement should not
  // cause the following low-power attempt to overestimate its link margin.
  const lastPathSnr = Number.isFinite(history.lastModeledSnrDb) ? history.lastModeledSnrDb : modeledSnr;
  const measured = Number.isFinite(lastSinrDb)
    ? 0.5 * clamp(lastSinrDb - (Number.isFinite(lastPowerGainDb) ? lastPowerGainDb : 0) - lastPathSnr, -12, 8)
    : 0;
  const recentOutcome = typeof lastDelivered === "boolean" ? (lastDelivered ? .5 : -.5) : 0;
  return weight * (delivery + measured + recentOutcome);
}

function option(candidate, settings) {
  const { elapsedS, goal, lastBand, lastPowerW, referenceWatts, retry } = settings;
  const sameFailedBand = retry > 0 && lastBand === candidate.band;
  const minimumPower = sameFailedBand
    ? POWER_LEVELS_W.find((power) => power > (lastPowerW ?? POWER_LEVELS_W[0])) ?? POWER_LEVELS_W.at(-1)
    : POWER_LEVELS_W[0];
  const estimatedAtReference = candidate.snrDb + historyCorrection(candidate.history, candidate.snrDb, elapsedS);
  const gain = (power) => 10 * Math.log10(power / referenceWatts);
  const powerW = POWER_LEVELS_W.find((power) => power >= minimumPower && estimatedAtReference + gain(power) >= goal)
    ?? POWER_LEVELS_W.at(-1);
  const powerGainDb = gain(powerW);
  const estimatedSinrDb = estimatedAtReference + powerGainDb;
  const margin = estimatedSinrDb - goal;
  const score = Math.min(margin, 2) - 0.13 * (10 * Math.log10(powerW / POWER_LEVELS_W[0]))
    - (sameFailedBand ? 2.5 : 0);
  return { candidate, powerW, powerGainDb, estimatedSinrDb, margin, score };
}

function compare(left, right) {
  return right.score - left.score || left.powerW - right.powerW
    || right.estimatedSinrDb - left.estimatedSinrDb
    || Number(left.candidate.band > right.candidate.band) - Number(left.candidate.band < right.candidate.band);
}

function wait(selected, reason, remaining) {
  const candidate = selected?.candidate ?? null;
  const availability = candidate?.availableInS;
  const desired = Number.isFinite(availability) && availability > 0 ? availability : DEFAULT_WAIT_S;
  return {
    action: "wait", band: candidate?.band ?? null, powerW: null, powerGainDb: 0,
    predictedSnrDb: null, estimatedSinrDb: null,
    waitS: Number.isFinite(remaining) && remaining > 0
      ? Math.min(remaining, clamp(desired, MIN_WAIT_S, LISTEN_WINDOW_S))
      : clamp(desired, MIN_WAIT_S, LISTEN_WINDOW_S),
    reason, candidate,
  };
}

/**
 * Each candidate supplies modeled SNR at referenceWatts before any tuning
 * gain, busy state, optional seconds until idle, and directional band history.
 * The target is a simulator policy threshold. The modem determines delivery.
 */
export function chooseRadioTuning({
  candidates = [],
  lastBand = null,
  lastPowerW = null,
  retry = 0,
  referenceWatts = 100,
  targetSnrDb = DEFAULT_TARGET_DB,
  elapsedS = 0,
  deadlineS = null,
}) {
  const watts = Number.isFinite(referenceWatts) && referenceWatts > 0 ? referenceWatts : 100;
  const target = Number.isFinite(targetSnrDb) ? targetSnrDb : DEFAULT_TARGET_DB;
  const goal = target + clamp(retry, 0, 2) * 1.5;
  const remaining = Number.isFinite(deadlineS) ? deadlineS - elapsedS : Infinity;
  const settings = { elapsedS, goal, lastBand, lastPowerW, referenceWatts: watts, retry };
  const ranked = candidates
    .filter((candidate) => typeof candidate?.band === "string" && Number.isFinite(candidate.snrDb))
    .map((candidate) => option(candidate, settings))
    .sort(compare);
  const idle = ranked.find(({ candidate }) => candidate.busy === false);
  if (!idle) {
    const next = ranked.find(({ candidate }) => candidate.availableInS > 0 && candidate.availableInS <= LISTEN_WINDOW_S)
      ?? ranked[0];
    const reason = next ? `Listening for a clear ${bandLabel(next.candidate.band)} channel` : "Waiting for a channel estimate";
    return wait(next, reason, remaining);
  }

  if (idle.margin < 0 && remaining > 0) {
    const next = ranked.find(({ candidate, margin }) => candidate.busy && candidate.availableInS > 0
      && candidate.availableInS <= Math.min(LISTEN_WINDOW_S, remaining) && margin >= 0);
    if (next) return wait(next, `Waiting for the stronger ${bandLabel(next.candidate.band)} channel to clear`, remaining);
  }

  const { candidate, powerW, powerGainDb, estimatedSinrDb } = idle;
  const label = bandLabel(candidate.band);
  let reason = `Clear ${label} channel has the best usable link estimate`;
  if (retry > 0 && lastBand !== null && lastBand !== candidate.band) {
    reason = `Retuned to a clear ${label} channel after the failed ${bandLabel(lastBand)} burst`;
  } else if (retry > 0 && powerW > (lastPowerW ?? POWER_LEVELS_W[0])) {
    reason = `Raised power on the clear ${label} channel after the failed burst`;
  } else if (idle.margin < 0) {
    reason = `Trying the strongest clear ${label} channel with limited link margin`;
  }
  return {
    action: "transmit", band: candidate.band, powerW, powerGainDb,
    predictedSnrDb: round(candidate.snrDb + powerGainDb), estimatedSinrDb: round(estimatedSinrDb),
    waitS: 0, reason, candidate,
  };
}
