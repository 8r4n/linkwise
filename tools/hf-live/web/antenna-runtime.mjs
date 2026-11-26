import { linkBudget, pathGeometry } from "./antenna-model.mjs";

export const ANTENNA_LAB = Object.freeze({
  id: "antenna-lab",
  fixtureVersion: 1,
  seed: 42,
  source: "SIM-022",
  target: "SIM-038",
  band: "20m",
  powerW: 1,
  startUtc: "2026-11-26T05:36:00Z",
  playbackRate: 8,
  payloadId: "antenna-lab-v1-frame",
});

export const ANTENNA_LAB_ARCHIVE = Object.freeze({
  "forecast-5f529132e9340c98.bin.gz": "c4636d63d3400dbf1e74a9df656998ef73c9a9ebe719d095a34f448cd94def07",
  "forecast.json": "aa5405d959abd3b6e1b46dc6165f7478552720bc5a9423de0fbf556acf4b24e6",
  "weather.json": "7ebe87db880899cfe66e47aa1303e12170bb98435daefca87d8419fa0667e29c",
});

const wrap = degrees => (degrees % 360 + 360) % 360;
const difference = (from, to) => (to - from + 540) % 360 - 180;
const settled = (from, to) => Math.abs(difference(from, to)) < 1e-6;

export class AntennaRuntime {
  constructor(catalog, stations, configs = []) {
    this.catalog = catalog;
    this.profiles = new Map(catalog.profiles.map(profile => [profile.id, profile]));
    this.stations = stations;
    this.stationIds = new Map(stations.map(({ id }, index) => [id, index]));
    this.states = stations.map(() => ({
      txPort: "isotropic", rxPort: "isotropic", targetTxPort: "isotropic", targetRxPort: "isotropic",
      actualAzimuth: 0, targetAzimuth: 0, fixedAzimuth: 0, autoAim: false, lastTime: 0, switchUntil: 0,
      state: "ready", revision: 0, lastPublished: -Infinity,
    }));
    this.ports = catalog.profiles.map(profile => ({
      id: profile.id, name: profile.name, role: profile.role,
      bands: Object.keys(profile.bands), rotatable: profile.rotatable,
    }));
    for (const config of Array.isArray(configs) ? configs.slice(0, stations.length) : []) {
      if (config && typeof config.station === "string") this.configure(config, 0, true);
    }
  }

  station(index) {
    return this.states[index];
  }

  summary(index) {
    const state = this.states[index];
    const { txPort, rxPort, targetTxPort, targetRxPort, actualAzimuth, targetAzimuth, fixedAzimuth, autoAim, revision } = state;
    return { station: this.stations[index].id, txPort, rxPort, targetTxPort, targetRxPort,
      ports: this.ports, actualAzimuth, targetAzimuth, fixedAzimuth, state: state.state, autoAim, revision };
  }

  snapshot() {
    return { enabled: true, catalog: this.catalog, stations: this.stations.map((_, index) => this.summary(index)) };
  }

  validate({ txPort, rxPort, azimuth, fixedAzimuth, autoAim }) {
    const tx = this.profiles.get(txPort);
    if (txPort !== undefined && (!tx || tx.role !== "tx-rx")) {
      return "Choose a transmit-capable antenna for TX.";
    }
    if (rxPort !== undefined && !this.profiles.has(rxPort)) {
      return "Unknown receive antenna.";
    }
    if ([azimuth, fixedAzimuth].some(value => value !== undefined && (!Number.isFinite(value) || value < 0 || value > 360))) {
      return "Enter an azimuth from 0 to 360 degrees.";
    }
    if (autoAim !== undefined && typeof autoAim !== "boolean") {
      return "The automatic aiming flag must be true or false.";
    }
    return null;
  }

  configure(command, now, instant = false, automatic = false) {
    const { station, txPort, rxPort, azimuth, fixedAzimuth, autoAim } = command;
    const index = this.stationIds.get(station);
    if (index === undefined) return { station, error: "Unknown station." };
    const error = this.validate(command);
    if (error) return { ...this.summary(index), error };
    const state = this.states[index];
    this.advanceStation(index, now);
    const oldTx = state.targetTxPort;
    const oldRx = state.targetRxPort;
    const oldAzimuth = state.targetAzimuth;
    const oldFixedAzimuth = state.fixedAzimuth;
    const oldAutoAim = state.autoAim;
    state.targetTxPort = txPort ?? oldTx;
    state.targetRxPort = rxPort ?? oldRx;
    const rotatable = [state.targetTxPort, state.targetRxPort].some(id => this.profiles.get(id).rotatable);
    state.targetAzimuth = rotatable ? (azimuth === undefined ? oldAzimuth : wrap(azimuth)) : state.actualAzimuth;
    const installation = fixedAzimuth ?? (rotatable ? undefined : azimuth);
    if (installation !== undefined) state.fixedAzimuth = wrap(installation);
    state.autoAim = autoAim ?? oldAutoAim;
    if (state.targetTxPort !== oldTx || state.targetRxPort !== oldRx) {
      const txDelay = state.targetTxPort === state.txPort ? 0 : this.profiles.get(state.targetTxPort).switch_s;
      const rxDelay = state.targetRxPort === state.rxPort ? 0 : this.profiles.get(state.targetRxPort).switch_s;
      state.switchUntil = now + Math.max(txDelay, rxDelay);
    }
    if (!automatic && (oldTx !== state.targetTxPort || oldRx !== state.targetRxPort
        || oldAzimuth !== state.targetAzimuth || oldFixedAzimuth !== state.fixedAzimuth || oldAutoAim !== state.autoAim)) state.revision++;
    if (instant) {
      state.actualAzimuth = state.targetAzimuth;
      state.switchUntil = now;
    }
    this.advanceStation(index, now);
    return this.summary(index);
  }

  advanceStation(index, now) {
    const state = this.states[index];
    const elapsed = Math.max(0, now - state.lastTime);
    state.lastTime = now;
    if (now >= state.switchUntil) {
      state.txPort = state.targetTxPort;
      state.rxPort = state.targetRxPort;
    }
    const profiles = [state.targetTxPort, state.targetRxPort].map(id => this.profiles.get(id));
    const speeds = profiles.filter(profile => profile.rotatable).map(profile => profile.slew_deg_per_s);
    const delta = difference(state.actualAzimuth, state.targetAzimuth);
    const maxStep = speeds.length ? Math.min(...speeds) * elapsed : Infinity;
    state.actualAzimuth = Math.abs(delta) <= maxStep ? state.targetAzimuth
      : wrap(state.actualAzimuth + Math.sign(delta) * maxStep);
    state.state = now < state.switchUntil ? "switching"
      : settled(state.actualAzimuth, state.targetAzimuth) ? "ready" : "rotating";
  }

  advance(now, publish) {
    for (let index = 0; index < this.states.length; index++) {
      const entry = this.states[index];
      const prior = entry.state;
      this.advanceStation(index, now);
      if (entry.state !== prior || (entry.state !== "ready" && now - entry.lastPublished >= .5)) {
        entry.lastPublished = now;
        publish(this.summary(index));
      }
    }
  }

  endpoint(index, role) {
    const state = this.states[index];
    const profile = this.profiles.get(state[role + "Port"]);
    return { profile_id: profile.id, heading_deg: profile.rotatable ? state.actualAzimuth : state.fixedAzimuth };
  }

  budget(source, target, band, conductedPowerW, tx = this.endpoint(source, "tx"), rx = this.endpoint(target, "rx")) {
    const geometry = pathGeometry(this.stations[source], this.stations[target]);
    const result = linkBudget({ geometry, band, tx, rx, conductedPowerW, catalog: this.catalog });
    if (!result) return null;
    const endpoint = (value, bearing) => ({
      profileId: value.profile_id, port: value.profile_id, label: this.profiles.get(value.profile_id).name,
      headingDeg: value.heading_deg, bearingDeg: bearing, gainDb: value.gain_dbi,
      feedlineLossDb: value.feedline_loss_db ?? 0,
    });
    return {
      tx: endpoint(result.tx, geometry.tx_bearing_deg), rx: endpoint(result.rx, geometry.rx_bearing_deg),
      distanceKm: geometry.distance_km, elevationDeg: geometry.elevation_deg,
      polarizationLossDb: result.polarization_loss_db, netGainDb: result.net_gain_db,
      conductedW: result.conducted_power_w, eirpW: result.eirp_w,
    };
  }

  plan(source, target, band, retry = 0) {
    const geometry = pathGeometry(this.stations[source], this.stations[target]);
    const candidates = (index, role, bearing) => {
      const state = this.states[index];
      const current = state[role + "Port"];
      const allowed = profile => Object.hasOwn(profile.bands, band) && (role === "rx" || profile.role === "tx-rx");
      const selected = this.profiles.get(current);
      const automatic = state.autoAim && (retry > 0 || !allowed(selected));
      const profiles = automatic
        ? this.catalog.profiles.filter(allowed) : allowed(selected) ? [selected] : [];
      return profiles.map(profile => ({
        profile_id: profile.id,
        heading_deg: profile.rotatable ? (automatic && bearing !== null ? bearing : state.actualAzimuth) : state.fixedAzimuth,
      }));
    };
    const transmit = candidates(source, "tx", geometry.tx_bearing_deg);
    const receive = candidates(target, "rx", geometry.rx_bearing_deg);
    let best = null;
    for (const tx of transmit) {
      for (const rx of receive) {
        const result = linkBudget({ geometry, band, tx, rx, conductedPowerW: 100, catalog: this.catalog });
        if (!result) continue;
        const delay = this.adjustmentDelay(source, "tx", tx) + this.adjustmentDelay(target, "rx", rx);
        const score = result.net_gain_db - .15 * delay;
        if (best === null || score > best.score) best = { tx, rx, gainDb: result.net_gain_db, score };
      }
    }
    return best;
  }

  adjustmentDelay(index, role, endpoint) {
    const state = this.states[index];
    const profile = this.profiles.get(endpoint.profile_id);
    const switching = endpoint.profile_id !== state[role + "Port"] ? profile.switch_s : 0;
    const rotating = profile.rotatable
      ? Math.abs(difference(state.actualAzimuth, endpoint.heading_deg)) / profile.slew_deg_per_s : 0;
    return Math.max(switching, rotating);
  }

  prepare(source, target, plan, now, publish) {
    let ready = true;
    for (const [index, role, endpoint] of [[source, "tx", plan.tx], [target, "rx", plan.rx]]) {
      const state = this.states[index];
      const rotatable = this.profiles.get(endpoint.profile_id).rotatable;
      if (endpoint.profile_id !== state[role + "Port"] || (rotatable && !settled(state.actualAzimuth, endpoint.heading_deg))) {
        const command = { station: this.stations[index].id, [role + "Port"]: endpoint.profile_id };
        if (rotatable) command.azimuth = wrap(endpoint.heading_deg);
        publish(this.configure(command, now, false, true));
        ready = false;
      }
      ready = ready && state.state === "ready";
    }
    return ready;
  }
}
