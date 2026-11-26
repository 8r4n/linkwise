import { analyze, loadModem } from "./browser-dsp.js";
import { loadPathInputs } from "./data/path-inputs.mjs";
import { readBytes, sha256 } from "./data/weather-inputs.mjs";
import { createStationMessaging } from "./station-messaging.js";
import { chooseRadioTuning } from "./radio-policy.js";
import { getTrainingScenario, TRAINING_ARCHIVE } from "./training-scenarios.mjs";
import { loadAntennaCatalog, pathGeometry } from "./antenna-model.mjs";
import { AntennaRuntime, ANTENNA_LAB, ANTENNA_LAB_ARCHIVE } from "./antenna-runtime.mjs";

const TICK_MS = 100;
const MAX_QUEUE = 6;
const MAX_RETRIES = 2;
const MAX_USER_RETRIES = 4;
const REACCESS_MEAN_S = 8;
const BAND_CAPACITY = 2;
const PCM_CAPACITY = 12;
const PAST_CAPTURES = 16;
const RECENT_EVENTS = 32;
const RADIO_HISTORY = 512;
const REFERENCE_WATTS = 100;
const PROFILE = "ITU-R F.1487 mid-latitude moderate";
const CHANNEL_MODEL = "watterson-moderate";
const LAB_PAYLOAD = Uint8Array.of(0, 0, 0, 1, 22, 38, 1, 0, 1);

function random(seed) {
  return () => {
    seed = (seed + 0x6d2b79f5) >>> 0;
    let bit = Math.imul(seed ^ (seed >>> 15), seed | 1);
    bit ^= bit + Math.imul(bit ^ (bit >>> 7), bit | 61);
    return ((bit ^ (bit >>> 14)) >>> 0) / 0x100000000;
  };
}

function hash(seed, ...parts) {
  let word = seed ^ 0x811c9dc5;
  for (const part of parts) {
    const label = String(part);
    for (let index = 0; index < label.length; index++) word = Math.imul(word ^ label.charCodeAt(index), 0x01000193);
    word = Math.imul(word ^ 0xff, 0x01000193);
  }
  return word >>> 0;
}

function slowOffset(seed, source, target, band, time) {
  const period = Math.floor(time / 300);
  const phase = (time % 300) / 300;
  const gaussian = tick => {
    const next = random(hash(seed, source, target, band, tick));
    return 5.5 * Math.sqrt(-2 * Math.log(1 - next())) * Math.cos(2 * Math.PI * next());
  };
  return (1 - phase) * gaussian(period) + phase * gaussian(period + 1);
}

function payloadFor(message) {
  const bytes = new Uint8Array(9);
  const view = new DataView(bytes.buffer);
  view.setUint32(0, message.id);
  bytes[4] = message.source + 1;
  bytes[5] = message.target + 1;
  bytes[6] = 1;
  view.setUint16(7, message.value);
  return bytes;
}

function roundDb(value) {
  return Math.round(value * 100) / 100;
}

function magnitudeRatio(signal, impairment) {
  const floor = 1e-30;
  return roundDb(10 * Math.log10(Math.max(signal, floor) / Math.max(impairment, floor)));
}

function defaultAntennaConfigs(stations, seed, overrides) {
  const byStation = new Map((Array.isArray(overrides) ? overrides.slice(0, stations.length) : [])
    .filter(value => value && typeof value.station === "string").map(value => [value.station, value]));
  const transmit = ["t2fd", "dipole-hf", "vertical-hf", "t2fd", "dipole-hf", "vertical-hf", "yagi-20m"];
  const receive = ["t2fd", "dipole-hf", "active-loop", "beverage", "vertical-hf", "tci-530"];
  return stations.map(({ id: station }, index) => {
    const txPort = transmit[(index + seed % transmit.length) % transmit.length];
    return {
      station, txPort, rxPort: receive[hash(seed, "receive antenna", station) % receive.length],
      azimuth: hash(seed, "mount azimuth", station) % 360,
      fixedAzimuth: hash(seed, "installed antenna azimuth", station) % 360, autoAim: txPort === "yagi-20m",
      ...byStation.get(station),
    };
  });
}

async function loadTrainingInputs(scenario, signal) {
  const antennaLab = scenario.id === ANTENNA_LAB.id;
  const archiveFiles = antennaLab ? ANTENNA_LAB_ARCHIVE : TRAINING_ARCHIVE;
  const version = antennaLab ? `antenna-v${scenario.fixtureVersion}` : `v${scenario.version}`;
  const directory = new URL(`./training-data/${version}/`, import.meta.url);
  const records = await Promise.all(Object.entries(archiveFiles).map(async ([file, digest]) => {
    const bytes = await readBytes(new URL(file, directory), 128000, { signal });
    if (await sha256(bytes) !== digest) throw Error("The training archive did not match its published checksum.");
    return [new URL(file.replace(/\.gz$/, ""), directory).href, { bytes, gzip: file.endsWith(".gz") }];
  }));
  const archive = new Map(records);
  const inputs = await loadPathInputs(directory, { signal, refresh: false, fetcher: async url => {
    const file = archive.get(url.href);
    if (!file) return new Response("", { status: 404 });
    const body = new Blob([file.bytes]).stream();
    return new Response(file.gzip ? body.pipeThrough(new DecompressionStream("gzip")) : body);
  } });
  const now = Date.parse(scenario.startUtc);
  const { forecast, weather } = inputs.status(now);
  if (forecast?.status !== "current" || forecast.rows !== (antennaLab ? 3 : 6) || weather.mode !== "live") {
    throw Error("The fixed forecast or NOAA input for this lesson is unavailable.");
  }
  return inputs;
}

class Simulation {
  constructor(modem, inputs, seed, bandwidth, emit, scenario, catalog, antennaConfigs) {
    this.modem = modem;
    this.inputs = inputs;
    this.stations = inputs.stations;
    this.bands = inputs.bands;
    this.scenario = scenario;
    this.antennaLab = scenario?.id === ANTENNA_LAB.id;
    this.antennas = catalog ? new AntennaRuntime(catalog, this.stations,
      this.antennaLab ? [] : defaultAntennaConfigs(this.stations, seed, antennaConfigs)) : null;
    this.trial = null;
    this.antennaTime = 0;
    this.antennaCommands = new Map();
    this.scenarioTime = scenario ? Date.parse(scenario.startUtc) : null;
    this.seed = seed;
    this.bandwidth = bandwidth;
    this.forecastWatts = inputs.status(this.scenarioTime ?? undefined).forecast?.assumptions?.tx_power_w ?? REFERENCE_WATTS;
    this.emit = emit;
    this.running = true;
    this.elapsed = 0;
    this.tick = 0;
    this.trafficRandom = random(seed);
    this.policyRandom = random(seed ^ 0xa81b);
    this.accessRandom = random(seed ^ 0x5afc);
    this.nextGeneration = this.stations.map(() => this.trafficRandom() * 4);
    this.queues = this.stations.map(() => []);
    this.active = new Map();
    this.latest = new Map();
    this.previous = [];
    this.pairs = new Map();
    this.pcm = new Map();
    this.recent = [];
    this.bandAccess = this.bands.map(() => 0);
    this.attemptsByBand = this.stations.map(() => this.bands.map(() => 0));
    this.successesByBand = this.stations.map(() => this.bands.map(() => 0));
    this.counts = { generated: 0, attempted: 0, delivered: 0, failed: 0, retried: 0, dropped: 0, aborted: 0, overlapped: 0 };
    this.userFrames = new Set();
    this.messageAttempts = new Map();
    this.radioHistory = new Map();
    this.messaging = createStationMessaging({
      emit: value => this.stationMessage(value),
      sendFrame: (source, target, payload, metadata) => this.sendUserFrame(source, target, payload, metadata),
      frameBytes: modem.payloadCapacity,
    });
  }

  now() {
    return this.scenarioTime === null ? Date.now() : this.scenarioTime + Math.round(this.elapsed * 1000);
  }

  stats() {
    return {
      ...this.counts,
      active: this.active.size,
      queued: this.queues.reduce((count, queue) => count + queue.length, 0),
    };
  }

  status() {
    const { forecast, weather } = this.inputs.status(this.now());
    return {
      running: this.running,
      error: null,
      elapsed_s: this.elapsed,
      stats: this.stats(),
      medium: "shared",
      interference: "none",
      forecast,
      weather: this.scenario ? { ...weather, origin: "archive" } : weather,
      scenario: this.scenario,
    };
  }

  snapshot() {
    const source = this.stations.find(station => station.id === ANTENNA_LAB.source);
    const target = this.stations.find(station => station.id === ANTENNA_LAB.target);
    const geometry = this.antennaLab ? pathGeometry(source, target) : null;
    return {
      stations: this.stations,
      bands: this.bands,
      active: [...this.active.values()].map(pair => pair.attempt),
      recent: this.recent,
      seed: this.seed,
      channel: `Linkwise BW${this.bandwidth} · WebAssembly`,
      bandwidth_hz: this.bandwidth,
      channel_model: CHANNEL_MODEL,
      sample_rate_hz: this.modem.rate,
      ...(this.antennas ? { antennas: this.antennas.snapshot() } : {}),
      ...(geometry ? { antennaLab: { ...ANTENNA_LAB, txBearingDeg: geometry.tx_bearing_deg, rxBearingDeg: geometry.rx_bearing_deg } } : {}),
      ...this.status(),
    };
  }

  notify(attempt) {
    this.emit("attempt", { attempt, stats: this.stats(), elapsed_s: this.elapsed });
  }

  stationMessage(value) {
    const attempts = Array.from(this.messageAttempts.get(value.id)?.values() ?? []);
    const terminal = value.state === "delivered" || value.state === "failed";
    const samples = attempts.reduce((total, row) => total + row.transmittedSamples, 0);
    this.emit("station-message", {
      ...value,
      radioEvidence: {
        scenarioId: this.scenario?.id ?? null, scenarioVersion: this.scenario?.version ?? null,
        bandwidthHz: this.bandwidth, frameCount: new Set(attempts.map(row => row.frameNumber)).size,
        airtimeS: samples / this.modem.rate, retries: attempts.filter(row => row.retry > 0).length,
        outcome: terminal ? value.state : "pending",
        attempts: attempts.map(({ transmittedSamples, ...row }) => ({
          ...row, airtimeS: transmittedSamples / this.modem.rate,
        })),
      },
    });
    if (terminal) this.messageAttempts.delete(value.id);
  }

  recordMessageAttempt(message, attempt, state, transmittedSamples) {
    const { id, frameNumber } = message.metadata ?? {};
    if (typeof id !== "string" || !Number.isInteger(frameNumber)) return;
    if (!this.messageAttempts.has(id)) this.messageAttempts.set(id, new Map());
    const entries = this.messageAttempts.get(id);
    const pending = entries.get(attempt.id);
    entries.set(attempt.id, {
      attemptId: attempt.id, frameNumber, band: attempt.band,
      frequencyMHz: this.bands.find(band => band.id === attempt.band)?.frequency_mhz,
      powerW: attempt.power_w, retry: attempt.retry, state, predictedSnrDb: attempt.snr_db,
      measuredSinrDb: attempt.measured_sinr_db,
      ...(attempt.antenna ? { antenna: attempt.antenna } : {}),
      transmittedSamples: transmittedSamples ?? pending?.transmittedSamples ?? 0,
    });
  }

  sendMessage(request) {
    if (this.antennaLab) {
      this.stationMessage({ id: request?.id, from: request?.from, to: request?.to, kind: request?.kind,
        state: "failed", error: "Use the Antenna Lab trial controls." });
      return;
    }
    if (this.scenario && (request?.from !== this.scenario.source || request?.to !== this.scenario.target)) {
      this.stationMessage({ id: request?.id, from: request?.from, to: request?.to, kind: request?.kind,
                            state: "failed", error: "Use the preset station pair for this lesson." });
      return;
    }
    return this.messaging.send(request);
  }

  sendUserFrame(sourceId, targetId, payload, metadata) {
    const source = this.stations.findIndex(station => station.id === sourceId);
    const target = this.stations.findIndex(station => station.id === targetId);
    if (source < 0 || target < 0 || source === target || !(payload instanceof Uint8Array)
        || !payload.length || payload.length > this.modem.payloadCapacity) return Promise.resolve(null);
    const frame = new Uint8Array(this.bandwidth === 500 ? this.modem.payloadCapacity : payload.length);
    frame.set(payload);
    return new Promise(resolve => {
      const message = {
        id: ++this.counts.generated, source, target, payload: frame, userLength: payload.length,
        retry: 0, ready: this.elapsed, resolve, metadata, deadline: this.elapsed + 10,
      };
      if (this.enqueuePriority(message)) this.userFrames.add(message);
      else resolve(null);
    });
  }

  enqueuePriority(message) {
    const queue = this.queues[message.source];
    const background = queue.findIndex(item => !item.payload);
    if (queue.length >= MAX_QUEUE) {
      if (background < 0) {
        this.counts.dropped++;
        return false;
      }
      queue.splice(background, 1);
      this.counts.dropped++;
    }
    queue.splice(background < 0 ? queue.length : background, 0, message);
    return true;
  }

  settleFrame(message, value) {
    if (this.userFrames.delete(message)) message.resolve(value);
  }

  radio(message, state, tuning = {}, measurements = {}) {
    const { band, powerW, reason, predictedSnrDb } = tuning;
    if (state === "waiting" && message.lastWait === reason) return;
    message.lastWait = state === "waiting" ? reason : null;
    const frequency = this.bands.find(candidate => candidate.id === band)?.frequency_mhz;
    this.emit("station-radio", {
      ...message.metadata,
      state, from: this.stations[message.source].id, to: this.stations[message.target].id,
      band, frequency_mhz: frequency, power_w: powerW, reason,
      dial_frequency_mhz: Number.isFinite(frequency) ? frequency - .0015 : undefined,
      retry: message.retry, attempt: message.retry + 1, mode: `BW${this.bandwidth}`,
      predicted_snr_db: predictedSnrDb, ...measurements,
    });
  }

  setAntenna(command) {
    const station = typeof command?.station === "string" ? command.station : "";
    if (!this.antennas || this.antennaLab) {
      this.emit("station-antenna", { station, error: "Antenna controls are unavailable in this run." });
      return;
    }
    const index = this.antennas.stationIds.get(station);
    if (index === undefined) {
      this.emit("station-antenna", { station, error: "Unknown station." });
      return;
    }
    const next = { ...this.antennaCommands.get(index), ...command };
    const error = this.antennas.validate(next);
    if (error) {
      this.emit("station-antenna", { ...this.antennas.summary(index), error });
      return;
    }
    if (this.stationBusy(index)) {
      this.antennaCommands.set(index, next);
      const state = this.antennas.summary(index);
      this.emit("station-antenna", {
        ...state, state: "queued", targetTxPort: next.txPort ?? state.targetTxPort,
        targetRxPort: next.rxPort ?? state.targetRxPort, targetAzimuth: next.azimuth ?? state.targetAzimuth,
      });
      return;
    }
    this.antennaCommands.delete(index);
    this.emit("station-antenna", this.antennas.configure(next, this.elapsed));
  }

  stationBusy(index) {
    return [...this.active.values()].some(({ message }) => message.source === index || message.target === index);
  }

  applyAntennaCommands() {
    for (const [index, command] of this.antennaCommands) {
      if (this.stationBusy(index)) continue;
      this.antennaCommands.delete(index);
      this.emit("station-antenna", this.antennas.configure(command, this.elapsed));
    }
  }

  historyKey(source, target, band) {
    const route = `${source}:${target}:${band}`;
    return this.antennas
      ? `${route}:${this.antennas.station(source).revision}:${this.antennas.station(target).revision}` : route;
  }

  runAntennaTrial(request) {
    const id = typeof request?.id === "string" && request.id.length <= 80 ? request.id : "";
    const reject = error => this.emit("antenna-trial", { id, state: "error", error });
    if (!this.antennaLab || !this.antennas) return reject("Open the Antenna Lab to run a matched trial.");
    if (!id || this.trial) return reject("Wait for the current trial to finish.");
    if (!this.running) return reject("Resume the simulation before running a trial.");
    const { source: txStation, target: rxStation, band, powerW } = ANTENNA_LAB;
    const source = this.antennas.stationIds.get(txStation);
    const target = this.antennas.stationIds.get(rxStation);
    const geometry = pathGeometry(this.stations[source], this.stations[target]);
    const tx = { profile_id: request.txPort ?? "yagi-20m", heading_deg: request.txAzimuth ?? geometry.tx_bearing_deg };
    const rx = { profile_id: request.rxPort ?? "yagi-20m", heading_deg: request.rxAzimuth ?? geometry.rx_bearing_deg };
    if ([tx, rx].some(endpoint => !Number.isFinite(endpoint.heading_deg) || endpoint.heading_deg < 0 || endpoint.heading_deg > 360)
        || !this.antennas.budget(source, target, band, powerW, tx, rx)) {
      return reject("Choose compatible 20 m antenna ports and valid azimuths.");
    }
    const match = {
      fixtureVersion: ANTENNA_LAB.fixtureVersion, seed: ANTENNA_LAB.seed, band, powerW, startedAtS: 0,
      payloadId: ANTENNA_LAB.payloadId, source: txStation, target: rxStation, startUtc: ANTENNA_LAB.startUtc,
    };
    this.trial = { id, match, comparisonKey: JSON.stringify(match), source, target };
    this.emit("station-antenna", this.antennas.configure({
      station: txStation, txPort: tx.profile_id, azimuth: tx.heading_deg,
    }, this.antennaTime));
    this.emit("station-antenna", this.antennas.configure({
      station: rxStation, rxPort: rx.profile_id, azimuth: rx.heading_deg,
    }, this.antennaTime));
    this.sendTrial("rotating");
  }

  sendTrial(state, attempt = null) {
    const { id, match, comparisonKey } = this.trial;
    this.emit("antenna-trial", { id, state, match, comparisonKey, ...(attempt ? { attemptId: attempt.id, attempt } : {}) });
  }

  advanceTrial() {
    if (!this.trial) return;
    this.antennaTime += TICK_MS / 1000;
    this.antennas.advance(this.antennaTime, value => this.emit("station-antenna", value));
    const { source, target } = this.trial;
    if (this.trial.started) {
      this.elapsed = this.tick++ * TICK_MS / 1000;
      this.complete();
      if (this.tick % (1000 / TICK_MS) === 0 || !this.trial) this.emit("status", this.status());
    } else if (this.antennas.station(source).state === "ready" && this.antennas.station(target).state === "ready") {
      this.elapsed = this.tick = 0;
      const message = { id: ++this.counts.generated, source, target, payload: LAB_PAYLOAD, retry: 0, trial: true };
      const band = this.bands.findIndex(value => value.id === ANTENNA_LAB.band);
      this.trial.started = true;
      this.sendTrial("transmitting", this.openBurst(message, band, null, this.scenarioTime));
    }
  }

  chooseTuning(message, available, using, now) {
    const { source, target, retry, lastBand, lastPowerW } = message;
    return chooseRadioTuning({
      candidates: this.bands.flatMap(({ id }, band) => {
        const antennaPlan = this.antennas?.plan(source, target, id, retry);
        if (this.antennas && !antennaPlan) return [];
        return [{
          band: id,
          snrDb: this.path(source, target, band, this.elapsed, now).snrDb + (antennaPlan?.gainDb ?? 0),
          busy: using[band] > 0 || !available.includes(band),
          availableInS: Math.max(0, ...[...this.active.values()]
            .filter(pair => pair.transmission.band === band).map(pair => pair.end - this.elapsed)),
          history: this.radioHistory.get(this.historyKey(source, target, id)),
          antennaPlan,
        }];
      }),
      lastBand, lastPowerW, retry, referenceWatts: REFERENCE_WATTS,
      elapsedS: this.elapsed, deadlineS: message.deadline,
    });
  }

  recordTuning(message, tuning, delivered, measuredSinr) {
    const key = this.historyKey(message.source, message.target, tuning.band);
    const previous = this.radioHistory.get(key);
    this.radioHistory.delete(key);
    this.radioHistory.set(key, {
      attempts: (previous?.attempts ?? 0) + 1,
      deliveries: (previous?.deliveries ?? 0) + Number(delivered),
      lastAtS: this.elapsed, lastSinrDb: measuredSinr,
      lastPowerGainDb: tuning.powerGainDb, lastDelivered: delivered,
      lastModeledSnrDb: tuning.predictedSnrDb - tuning.powerGainDb,
    });
    if (this.radioHistory.size > RADIO_HISTORY) this.radioHistory.delete(this.radioHistory.keys().next().value);
  }

  close() {
    this.running = false;
    for (const pair of this.active.values()) {
      if (pair.tuning) {
        const samples = Math.min(
          Math.round(pair.attempt.airtime_s * this.modem.rate),
          Math.round((this.elapsed - pair.transmission.started) * this.modem.rate),
        );
        this.recordMessageAttempt(pair.message, pair.attempt, "aborted", samples);
      }
    }
    for (const message of this.userFrames) this.settleFrame(message, null);
    this.messaging.close();
  }

  generate() {
    for (let source = 0; source < this.stations.length; source++) {
      if (this.nextGeneration[source] > this.elapsed) continue;
      this.nextGeneration[source] = this.elapsed + 10 + 17 * this.trafficRandom();
      let target = Math.floor(this.trafficRandom() * (this.stations.length - 1));
      if (target >= source) target++;
      const message = {
        id: ++this.counts.generated,
        source, target,
        value: Math.floor(this.trafficRandom() * 65536),
        retry: 0,
        ready: this.elapsed,
      };
      const queue = this.queues[source];
      if (queue.length < MAX_QUEUE) queue.push(message);
      else this.counts.dropped++;
    }
  }

  path(source, target, band, started, now) {
    const bandId = this.bands[band].id;
    const variation = slowOffset(this.seed, source, target, bandId, started);
    const path = this.inputs.conditions(this.stations[source], this.stations[target], bandId, started, variation, now);
    const forecast = path.details?.forecast?.status;
    if (forecast === "predicted" || forecast === "archived") {
      // Put P.533 predictions and the 100 W heuristic on a common reference.
      path.snrDb += 10 * Math.log10(REFERENCE_WATTS / this.forecastWatts);
    }
    return path;
  }

  writePcm(id, entry) {
    this.pcm.delete(id);
    this.pcm.set(id, entry);
    if (this.pcm.size > PCM_CAPACITY) this.pcm.delete(this.pcm.keys().next().value);
  }

  txSamples(transmission) {
    const previous = this.pcm.get(transmission.id);
    if (previous?.tx) {
      this.writePcm(transmission.id, previous);
      return previous.tx;
    }
    // Bounded PCM storage is independent from capture identity. The codec is
    // deterministic, so an older retained descriptor can reproduce its TX.
    const tx = this.modem.encode(transmission.payload);
    this.writePcm(transmission.id, { ...previous, tx });
    return tx;
  }

  retain(pair, tx) {
    const { source, id } = pair.transmission;
    const station = this.stations[source].id;
    const last = this.latest.get(station);
    if (last !== undefined) {
      this.previous.push(last);
      if (this.previous.length > PAST_CAPTURES) {
        const old = this.previous.shift();
        this.pairs.delete(old);
        this.pcm.delete(old);
      }
    }
    this.latest.set(station, id);
    this.pairs.set(id, pair);
    this.writePcm(id, { tx });
  }

  start() {
    const busy = new Set();
    const using = this.bands.map(() => 0);
    for (const pair of this.active.values()) {
      const { message, transmission } = pair;
      busy.add(message.source).add(message.target);
      using[transmission.band]++;
    }
    const available = this.bands.map((_, band) => band).filter(band =>
      using[band] < BAND_CAPACITY && (!using[band] || this.bandAccess[band] <= this.elapsed));
    const queued = this.queues.map(queue => queue[0]).filter(Boolean);
    const overdue = queued.filter(message => message.payload && message.deadline <= this.elapsed)
      .sort((a, b) => a.deadline - b.deadline || a.id - b.id)[0];
    if (overdue && overdue.listenBand == null) {
      overdue.listenBand = this.chooseTuning(overdue, available, using, this.now()).band;
    }
    let reserving = Boolean(overdue);
    const pending = queued.filter(message => message.ready <= this.elapsed || message === overdue)
      .sort((a, b) => Number(b === overdue) - Number(a === overdue)
        || Number(Boolean(b.payload)) - Number(Boolean(a.payload)) || a.ready - b.ready || a.id - b.id);

    for (const message of pending) {
      const { source, target } = message;
      if (this.antennas && (this.antennas.station(source).state !== "ready" || this.antennas.station(target).state !== "ready")) {
        if (message.payload) this.radio(message, "waiting", { reason: "Waiting for both antenna systems to settle." });
        continue;
      }
      const plans = this.antennas ? new Map(this.bands.flatMap(({ id }, index) => {
        const plan = this.antennas.plan(source, target, id, message.retry);
        return plan ? [[index, plan]] : [];
      })) : null;
      if (plans?.size === 0) {
        this.queues[source].shift();
        this.counts.dropped++;
        if (message.payload) {
          this.radio(message, "failed", { reason: "These antenna ports have no common supported band." });
          this.settleFrame(message, null);
        }
        if (message === overdue) reserving = false;
        continue;
      }
      // Let already-airborne calls finish while an overdue user frame listens.
      // Only new background calls yield the reserved band and stations.
      let eligible = available;
      if (reserving && !message.payload) {
        if ([source, target].some(station => station === overdue.source || station === overdue.target)) continue;
        eligible = available.filter(band => this.bands[band].id !== overdue.listenBand);
      }
      if (plans) eligible = eligible.filter(band => plans.has(band));
      if (!eligible.length || busy.has(source) || busy.has(target)) {
        if (message.payload) this.radio(message, "waiting", {
          band: message === overdue ? overdue.listenBand : undefined,
          reason: !eligible.length ? "Waiting for a clear band." : "Waiting for both stations to finish current traffic.",
        });
        continue;
      }
      let chosen = 0;
      let best = -Infinity;
      const now = this.now();
      let tuning;
      if (message.payload) {
        tuning = this.chooseTuning(message, eligible, using, now);
        if (tuning.action === "wait") {
          this.radio(message, "waiting", tuning);
          message.ready = this.elapsed + Math.max(.1, tuning.waitS ?? .4);
          continue;
        }
        chosen = eligible.findIndex(band => this.bands[band].id === tuning.band && !using[band]);
        if (chosen < 0) throw Error("The radio selected an occupied band.");
      } else {
        for (const [index, band] of eligible.entries()) {
          const score = (this.successesByBand[source][band] + 1) / (this.attemptsByBand[source][band] + 2)
            + this.policyRandom() * .4 + (plans?.get(band)?.gainDb ?? 0) / 20;
          if (score > best) {
            best = score;
            chosen = index;
          }
        }
      }
      const band = eligible[chosen];
      if (plans && !this.antennas.prepare(source, target, plans.get(band), this.elapsed, value => this.emit("station-antenna", value))) {
        busy.add(source).add(target);
        if (message.payload) this.radio(message, "waiting", {
          band: this.bands[band].id, reason: "Aiming or switching the antenna systems.",
        });
        continue;
      }
      available.splice(available.indexOf(band), 1);
      if (message === overdue) reserving = false;
      using[band]++;
      this.bandAccess[band] = this.elapsed + .2 - REACCESS_MEAN_S * Math.log(1 - this.accessRandom());
      busy.add(source).add(target);
      this.queues[source].shift();
      this.counts.retried += Number(message.retry > 0);
      this.attemptsByBand[source][band]++;
      this.openBurst(message, band, tuning, now);
    }
  }

  openBurst(message, band, tuning, now) {
    const { source, target } = message;
    const { snrDb, details } = this.path(source, target, band, this.elapsed, now);
    const powerW = message.trial ? ANTENNA_LAB.powerW : tuning?.powerW ?? REFERENCE_WATTS;
    const gainDb = message.trial ? 10 * Math.log10(powerW / REFERENCE_WATTS) : tuning?.powerGainDb ?? 0;
    const txAntenna = this.antennas?.endpoint(source, "tx");
    const rxAntenna = this.antennas?.endpoint(target, "rx");
    const budget = this.antennas?.budget(source, target, this.bands[band].id, powerW, txAntenna, rxAntenna);
    const combinedGain = gainDb + (budget?.netGainDb ?? 0);
    const antenna = budget ? {
      ...budget, referenceSnrDb: roundDb(snrDb), powerGainDb: roundDb(gainDb),
      predictedSnrDb: roundDb(snrDb + combinedGain),
    } : null;
    const id = ++this.counts.attempted;
    const payload = message.payload ?? payloadFor(message);
    const tx = this.modem.encode(payload);
    const airtime = tx.length / this.modem.rate;
    const transmission = {
      id, source, band, now, payload, started: this.elapsed, txAntenna,
      power: this.modem.power(tx), gainDb,
      seed: message.trial ? hash(this.seed, "antenna lab RF noise") : hash(this.seed, id, source),
    };
    const attempt = {
      id, message_id: message.id, source: this.stations[source].id, target: this.stations[target].id,
      band: this.bands[band].id, state: "active", decode_status: "pending", retry: message.retry,
      simulated_at_s: this.elapsed, started_at_s: this.elapsed, snr_db: roundDb(Math.max(-100, Math.min(100, snrDb + combinedGain))),
      airtime_s: airtime, payload_bytes: payload.length, channel_model: CHANNEL_MODEL, interference: "none",
      waveform_mode: `BW${this.bandwidth}`,
    };
    if (tuning || antenna) {
      attempt.power_w = powerW;
      attempt.power_gain_db = gainDb;
    }
    if (antenna) attempt.antenna = antenna;
    if (details) attempt.space_weather = details;
    const pair = { attempt, message, transmission, tuning, rxAntenna, baseSnrDb: snrDb,
      combinedGain, end: this.elapsed + airtime, overlaps: [], routes: null };
    for (const other of this.active.values()) {
      if (other.transmission.band !== band) continue;
      pair.overlaps.push(other.transmission);
      other.overlaps.push(transmission);
    }
    this.retain(pair, tx);
    this.active.set(id, pair);
    if (tuning) this.recordMessageAttempt(message, attempt, "transmitting", tx.length);
    this.notify(attempt);
    if (tuning) this.radio(message, "transmitting", tuning, { attempt_id: id, antenna });
    return attempt;
  }

  propagate(transmission, target, snr, noise) {
    const { band, source, started, seed } = transmission;
    const path = hash(this.seed, source, target, this.bands[band].id);
    return this.modem.propagate(
      this.txSamples(transmission), seed, path, Math.round(started * this.modem.rate), Math.max(-100, Math.min(100, snr)), noise,
    );
  }

  receive(pair) {
    const { transmission, message, attempt } = pair;
    const directional = Boolean(attempt.antenna);
    const noiseSnr = directional ? pair.baseSnrDb : attempt.snr_db;
    const { signal, noise, signalPower, noisePower } = this.propagate(transmission, message.target, noiseSnr, true);
    const interference = new Float32Array(signal.length);
    const signalGain = 10 ** (pair.combinedGain / 10);
    const noiseGain = directional ? 1 : signalGain;
    const amplitudeGain = Math.sqrt(signalGain);
    const noiseAmplitude = Math.sqrt(noiseGain);
    const noiseReference = transmission.power * noiseGain * 10 ** (-Math.max(-100, Math.min(100, noiseSnr)) / 10);
    const rate = this.modem.rate;
    for (const [index, other] of pair.overlaps.entries()) {
      const { signal: incoming } = this.propagate(other, message.target, pair.routes[index], false);
      const gain = Math.sqrt(noiseReference * 10 ** (pair.routes[index] / 10) / other.power);
      const offset = Math.round(other.started * rate) - Math.round(transmission.started * rate);
      const end = Math.min(incoming.length + offset, interference.length);
      for (let sample = Math.max(0, offset); sample < end; sample++) {
        interference[sample] += incoming[sample - offset] * gain;
      }
    }
    const interferencePower = pair.overlaps.length ? this.modem.power(interference) : 0;
    const rx = signal;
    // The antenna correction scales the wanted signal against unchanged
    // reference noise. Old neutral runs keep their existing normalization.
    for (let sample = 0; sample < rx.length; sample++) {
      rx[sample] = directional
        ? rx[sample] * amplitudeGain + noise[sample] * noiseAmplitude + interference[sample]
        : (rx[sample] + noise[sample]) * amplitudeGain + interference[sample];
    }
    this.writePcm(transmission.id, { ...this.pcm.get(transmission.id), rx });
    return {
      rx,
      snr: directional ? magnitudeRatio(signalPower * signalGain, noisePower * noiseGain)
        : magnitudeRatio(signalPower, noisePower),
      sinr: magnitudeRatio(signalPower * signalGain, noisePower * noiseGain + interferencePower),
    };
  }

  complete() {
    for (const [id, pair] of this.active) {
      if (pair.end > this.elapsed) continue;
      const { message, transmission, tuning } = pair;
      pair.routes = pair.overlaps.map(other => {
        const gain = other.txAntenna && pair.rxAntenna
          ? this.antennas.budget(other.source, message.target, this.bands[other.band].id,
            REFERENCE_WATTS, other.txAntenna, pair.rxAntenna)?.netGainDb ?? 0 : 0;
        return this.path(other.source, message.target, other.band, other.started, other.now).snrDb + other.gainDb + gain;
      });
      const { rx, snr, sinr } = this.receive(pair);
      const decoded = this.modem.decode(rx, transmission.payload);
      const delivered = decoded !== null;
      if (tuning) {
        this.recordTuning(message, tuning, delivered, sinr);
        message.lastBand = tuning.band;
        message.lastPowerW = tuning.powerW;
        this.radio(message, delivered ? "delivered" : "failed", tuning, {
          attempt_id: id, measured_snr_db: snr, measured_sinr_db: sinr,
          ...(pair.attempt.antenna ? { antenna: pair.attempt.antenna } : {}),
        });
      }
      this.active.delete(id);
      this.counts.overlapped += Number(pair.overlaps.length > 0);
      if (delivered) {
        this.counts.delivered++;
        this.successesByBand[message.source][transmission.band]++;
        if (message.payload) this.settleFrame(message, decoded.slice(0, message.userLength));
      } else {
        this.counts.failed++;
        const queue = this.queues[message.source];
        if (message.trial) {
          this.counts.dropped++;
        } else if (message.payload && message.retry < MAX_USER_RETRIES) {
          message.retry++;
          message.ready = this.elapsed + 1.5 + this.policyRandom() * 3.5;
          message.deadline = message.ready + 10;
          message.listenBand = null;
          if (!this.enqueuePriority(message)) this.settleFrame(message, null);
        } else if (!message.payload && message.retry < MAX_RETRIES && queue.length < MAX_QUEUE) {
          message.retry++;
          message.ready = this.elapsed + (1.5 + this.policyRandom() * 3.5) * message.retry;
          queue.unshift(message);
        } else {
          this.counts.dropped++;
          this.settleFrame(message, null);
        }
      }
      const state = delivered ? "delivered" : "failed";
      pair.attempt = {
        ...pair.attempt,
        state, decode_status: state, simulated_at_s: this.elapsed, profile: PROFILE,
        measured_snr_db: snr, measured_sinr_db: sinr, interferers: pair.overlaps.map(other => other.id),
      };
      if (tuning) this.recordMessageAttempt(message, pair.attempt, state);
      this.recent.unshift(pair.attempt);
      if (this.recent.length > RECENT_EVENTS) this.recent.pop();
      this.notify(pair.attempt);
      if (message.trial) {
        this.sendTrial(state, pair.attempt);
        this.trial = null;
      }
    }
  }

  advance() {
    if (this.antennaLab) {
      this.advanceTrial();
      return;
    }
    // The training clock advances only with an actual queued or active frame.
    // Operator interactions and cryptography scheduling cannot move the RF epoch.
    if (this.scenario && !this.userFrames.size) return;
    this.elapsed = this.tick * TICK_MS / 1000;
    this.antennas?.advance(this.elapsed, value => this.emit("station-antenna", value));
    this.complete();
    this.applyAntennaCommands();
    if (!this.scenario) this.generate();
    this.start();
    if (this.tick % (1000 / TICK_MS) === 0) this.emit("status", this.status());
    this.tick++;
  }

  capture({ station, stage, attempt, after }) {
    const id = attempt == null ? this.latest.get(station) : Number(attempt);
    const pair = this.pairs.get(id);
    if (!pair || pair.attempt.source !== station) return { status: 404 };
    if (String(id) === String(after) || (stage === "rx" && pair.attempt.state === "active")) return { status: 204 };
    const samples = stage === "tx" ? this.txSamples(pair.transmission)
      : this.pcm.get(id)?.rx ?? this.receive(pair).rx;
    const waveform = samples.slice();
    const analysis = analyze(samples, this.modem.rate);
    const value = {
      attempt: pair.attempt, stage, sample_count: waveform.length,
      sample_rate_hz: this.modem.rate, waveform_f32: waveform, analysis,
    };
    return { status: 200, value, transfer: [waveform.buffer, analysis.spectrum_dbfs.buffer, analysis.waterfall_dbfs.buffer] };
  }
}

const send = (type, value) => self.postMessage({ type, value });
const lifecycle = new AbortController();
let simulation;
let timer;
let nextTick;
let closed = false;

function fail(error) {
  clearTimeout(timer);
  lifecycle.abort();
  simulation?.close();
  self.postMessage({ type: "error", message: error.message || "The simulation could not continue." });
}

function pump() {
  try {
    simulation.advance();
    const rate = simulation.userFrames.size || simulation.trial ? simulation.scenario?.playbackRate ?? 1 : 1;
    nextTick = Math.max(nextTick + TICK_MS / rate, performance.now() - 200);
    timer = setTimeout(pump, Math.max(0, nextTick - performance.now()));
  } catch (error) {
    fail(error);
  }
}

self.addEventListener("message", async ({ data }) => {
  try {
    if (data.type === "close") {
      closed = true;
      clearTimeout(timer);
      lifecycle.abort();
      simulation?.close();
      self.postMessage({ type: "closed" });
    } else if (closed) {
      return;
    } else if (data.type === "start") {
      if (simulation) return;
      const antennaLab = data.scenarioId === ANTENNA_LAB.id;
      const scenario = antennaLab ? ANTENNA_LAB : getTrainingScenario(data.scenarioId);
      if (data.scenarioId != null && !scenario) throw Error("This training scenario is unavailable.");
      if (antennaLab && data.bandwidth !== 500) throw Error("The Antenna Lab requires BW500.");
      const [modem, inputs, antennaCatalog] = await Promise.all([
        loadModem(data.bandwidth),
        scenario ? loadTrainingInputs(scenario, lifecycle.signal)
          : loadPathInputs(new URL("./data/", import.meta.url), { signal: lifecycle.signal }),
        antennaLab || (!scenario && data.antennas) ? loadAntennaCatalog({ signal: lifecycle.signal }) : null,
      ]);
      if (closed) return;
      simulation = new Simulation(modem, inputs, scenario?.seed ?? (data.seed >>> 0),
        data.bandwidth, send, scenario, antennaCatalog, data.antennaConfigs);
      send("snapshot", simulation.snapshot());
      nextTick = performance.now();
      pump();
    } else if (data.type === "run" && simulation && simulation.running !== data.running) {
      simulation.running = data.running;
      clearTimeout(timer);
      send("status", simulation.status());
      if (simulation.running) {
        nextTick = performance.now();
        pump();
      }
    } else if (data.type === "capture") {
      const { status, value, transfer = [] } = simulation?.capture(data) ?? { status: 503 };
      self.postMessage({ type: "capture", id: data.id, status, value }, transfer);
    } else if (data.type === "send-message" && simulation) {
      await simulation.sendMessage(data.request);
    } else if (data.type === "set-antenna" && simulation) {
      simulation.setAntenna(data.command);
    } else if (data.type === "antenna-trial" && simulation) {
      simulation.runAntennaTrial(data.request);
    }
  } catch (error) {
    // A capture is optional, so an unavailable waveform should not take down
    // the entire station network.
    if (data.type === "capture") self.postMessage({ type: "capture", id: data.id, status: 500 });
    else if (!closed) fail(error);
  }
});
