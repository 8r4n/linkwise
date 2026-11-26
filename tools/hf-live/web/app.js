import { createWaveformPanel } from "./waveform.js";
import { createBrowserTransport } from "./browser-transport.js";
import { createStationConsole } from "./station-console.js";
import { createLearning } from "./learning-ui.mjs";
import { antennaReadout, createAntennaConsole, headingLabel, pointAlongHeading } from "./antenna-console.mjs";
import { mountAntennaLab } from "./antenna-lab.mjs";
import { gainCut, pathGeometry } from "./antenna-model.mjs";

const $ = (id) => document.getElementById(id);
const traffic = $("traffic");
const stationFilter = $("station-filter");
const bandFilter = $("band-filter");
const mapPatterns = $("map-patterns");
const states = { active: "Busy", delivered: "Delivered", failed: "Lost" };
const totals = ["attempted", "active", "delivered", "failed", "retried", "dropped", "queued"];
const number = new Intl.NumberFormat("en-US", { maximumFractionDigits: 3 });
const utcTime = new Intl.DateTimeFormat("en-GB", {
  day: "numeric", month: "short", hour: "2-digit", minute: "2-digit", hourCycle: "h23", timeZone: "UTC",
});
const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");
const touchPointer = matchMedia("(pointer: coarse)");
const active = new Map();
const rows = new Map();
let stations = new Map();
let bands = new Map();
let recent = [];
let stats = {};
let running = false;
let error = null;
let elapsed = 0;
let selected = null;
let transport = "connecting";
let synchronized = false;
let globe = null;
const inBrowser = document.documentElement.dataset.mode === "browser";
const query = new URL(location.href).searchParams;
const inAntennaLab = inBrowser && query.get("lab") === "antenna";
const antennaPreset = inBrowser && !inAntennaLab && !query.has("mission") && query.get("antennas") === "1";
const vendor = new URL("./vendor/", import.meta.url);
const modeControl = $("browser-mode");
const runControl = $("browser-run");
let browserTransport = null;
let antennaOverlay = null;
let antennaCatalog = null;
let antennaStations = new Map();
const waveform = createWaveformPanel({
  bandName, bandFrequencyHz, stationName, requestCapture, antennaName,
  onCapture: capture => learning?.onCapture(capture),
  lockLevels: inAntennaLab,
});
const stationConsole = inBrowser ? createStationConsole({ send: sendMessage }) : null;
const learning = inBrowser && !inAntennaLab ? createLearning({
  id: query.get("mission"),
  launch: bandwidth => { void startBrowserMode(bandwidth); },
  send: sendMessage,
  viewBurst: (station, attempt) => {
    stationFilter.value = station;
    renderTraffic();
    waveform.inspect(station, attempt);
  },
  stationName,
}) : null;
const antennaConsole = antennaPreset ? createAntennaConsole({
  transport: () => browserTransport,
  onStation: id => {
    stationFilter.value = id;
    changeStation();
  },
  onOverlay: value => {
    antennaOverlay = value;
    globe?.drawAntenna(value);
  },
}) : null;
const antennaLab = inAntennaLab ? mountAntennaLab({
  root: $("antenna-lab"), transport: () => browserTransport, learningHref: "../learn/",
  onCapture: (station, attempt) => {
    stationFilter.value = station;
    focusAttempt(String(attempt), true);
    waveform.inspect(station, attempt, "rx");
    $("waveform-title").focus({ preventScroll: true });
    $("waveform-title").scrollIntoView({ block: "start" });
  },
}) : null;

function sendMessage(request) {
  if (!browserTransport) throw Error("The browser simulation is starting.");
  return browserTransport.sendMessage(request);
}

function requestCapture(station, options) {
  if (inBrowser) {
    if (!browserTransport) return Promise.reject(Error("The browser simulation is starting."));
    return browserTransport.requestCapture(station, options);
  }
  const { stage, attempt, after, signal } = options;
  const url = new URL("./waveforms/" + encodeURIComponent(station), location.href);
  url.searchParams.set("stage", stage);
  if (attempt != null) url.searchParams.set("attempt", attempt);
  if (after != null) url.searchParams.set("after", after);
  return fetch(url, { cache: "no-store", signal });
}

function text(element, value) {
  if (element.textContent !== value) element.textContent = value;
}

function clock(seconds) {
  const whole = Math.floor(Math.max(0, seconds));
  return [Math.floor(whole / 3600), Math.floor(whole / 60) % 60, whole % 60]
    .map((part) => String(part).padStart(2, "0")).join(":");
}

function bandName(id) {
  const band = bands.get(id);
  if (!band) return id;
  return Number.isFinite(band.frequency_mhz)
    ? `${band.name} · ${number.format(band.frequency_mhz)} MHz`
    : band.name;
}

function bandFrequencyHz(id) {
  const mhz = bands.get(id)?.frequency_mhz;
  return Number.isFinite(mhz) ? Math.round(mhz * 1e6) : null;
}

function stationName(id) {
  return stations.get(id)?.name ?? id;
}

function antennaName(id) {
  return antennaCatalog?.profiles.find(profile => profile.id === id)?.name ?? id;
}

function setOptions(select, all, options) {
  const previous = select.value;
  select.replaceChildren(new Option(all, ""), ...options);
  if ([...select.options].some((option) => option.value === previous)) select.value = previous;
}

function normalizedAttempt(value) {
  if (!value || value.id == null || !Object.hasOwn(states, value.state)) return null;
  return {
    ...value,
    id: String(value.id),
    source: String(value.source),
    target: String(value.target),
    band: String(value.band),
  };
}

function setNotice(message) {
  const notice = $("feed-error");
  notice.hidden = !message;
  text(notice, message ?? "");
}

function renderStatus() {
  const indicator = $("connection");
  const state = transport === "open" && synchronized
    ? error ? ["error", "Simulation error"]
      : running ? ["live", inBrowser ? "Running in this browser" : "Live feed"]
      : ["stopped", inBrowser ? "Paused in this browser" : "Simulation stopped"]
    : transport === "reconnect" ? ["connecting", "Reconnecting"] : ["connecting", "Connecting"];
  if (inBrowser) {
    runControl.disabled = !synchronized || Boolean(error);
    text(runControl, running ? "Pause simulation" : "Resume simulation");
  }
  indicator.dataset.state = state[0];
  text(indicator, state[1]);
  setNotice(error);
  text($("sim-clock"), clock(elapsed));
  for (const key of totals) {
    text($(key), Number.isFinite(stats[key]) ? number.format(stats[key]) : "—");
  }
}

function visible(attempt) {
  return (!stationFilter.value || [attempt.source, attempt.target].includes(stationFilter.value))
    && (!bandFilter.value || attempt.band === bandFilter.value);
}

function recentTraffic() {
  return [...active.values()].reverse().concat(recent).filter(visible);
}

function makeRow(id) {
  const row = document.createElement("li");
  const button = document.createElement("button");
  const heading = document.createElement("span");
  const route = document.createElement("span");
  const state = document.createElement("span");
  const meta = document.createElement("span");
  button.type = "button";
  button.className = "traffic-button";
  heading.className = "traffic-title";
  route.className = "traffic-route";
  state.className = "traffic-state";
  meta.className = "traffic-meta";
  heading.append(route, state);
  button.append(heading, meta);
  row.append(button);
  button.addEventListener("click", () => focusAttempt(id));
  const entry = { row, button, route, state, meta };
  rows.set(id, entry);
  return entry;
}

function renderTraffic() {
  const attempts = recentTraffic();
  if (selected && !attempts.some((attempt) => attempt.id === selected)) selected = null;
  const ids = new Set(attempts.map((attempt) => attempt.id));
  for (const [id, entry] of rows) {
    if (!ids.has(id)) {
      entry.row.remove();
      rows.delete(id);
    }
  }
  attempts.forEach((attempt, i) => {
    const entry = rows.get(attempt.id) ?? makeRow(attempt.id);
    text(entry.route, `${stationName(attempt.source)} → ${stationName(attempt.target)}`);
    text(entry.state, states[attempt.state]);
    entry.button.dataset.kind = attempt.state;
    entry.button.setAttribute("aria-pressed", String(attempt.id === selected));
    const detail = [bandName(attempt.band), `M${attempt.message_id ?? "—"}`];
    if (attempt.retry > 0) detail.push(`retry ${attempt.retry}`);
    if (Number.isFinite(attempt.measured_sinr_db)) detail.push(`${number.format(attempt.measured_sinr_db)} dB measured SINR`);
    else if (Number.isFinite(attempt.snr_db)) detail.push(`${number.format(attempt.snr_db)} dB predicted SNR`);
    if (Number.isFinite(attempt.simulated_at_s)) detail.push(clock(attempt.simulated_at_s));
    text(entry.meta, detail.join(" · "));
    if (traffic.children[i] !== entry.row) traffic.insertBefore(entry.row, traffic.children[i] ?? null);
  });
  const empty = $("traffic-empty");
  empty.hidden = attempts.length > 0;
  text(empty, bandFilter.value || stationFilter.value ? "No calls match these filters." : "Waiting for the first call.");
  globe?.draw(attempts);
}

function drawAttemptAntenna(attempt) {
  const budget = antennaReadout(attempt?.antenna, antennaName);
  if (!budget) return;
  const { tx, rx } = budget;
  const source = stations.get(attempt.source);
  const peer = stations.get(attempt.target);
  if (!source || !peer) return;
  antennaOverlay = { source, peer, heading: tx.headingDeg, bearing: tx.bearingDeg, txProfile: tx.profileId,
    rxHeading: rx.headingDeg, rxProfile: rx.profileId,
    band: attempt.band, elevation: budget.elevationDeg };
  globe?.drawAntenna(antennaOverlay);
}

function applyStatus(value) {
  if (value.stats && typeof value.stats === "object") stats = value.stats;
  if (typeof value.running === "boolean") running = value.running;
  if (Object.hasOwn(value, "error")) error = value.error == null ? null : String(value.error);
  if (Number.isFinite(value.elapsed_s)) elapsed = value.elapsed_s;
  if (Object.hasOwn(value, "weather")) renderWeather(value.weather);
  if (Object.hasOwn(value, "forecast")) {
    const source = $("forecast-source");
    const forecast = value.forecast;
    source.hidden = !forecast?.engine;
    source.title = forecast?.revision ?? "";
    const label = forecast?.status === "archived" ? "Archived forecast" : "Forecast";
    text(source, forecast?.engine ? [
      `${label} · ${forecast.engine}`, forecast.release, forecast.month_utc,
    ].filter(Boolean).join(" · ") : "");
  }
}

function formatObservation(observation, value) {
  const parts = [];
  if (value != null) parts.push(number.format(value));
  const observed = Date.parse(observation?.observed_at ?? "");
  if (Number.isFinite(observed)) parts.push(`${utcTime.format(observed)} UTC`);
  const validity = observation?.status;
  if (validity && !["live", "fresh", "ok"].includes(validity)) parts.push(String(validity).replaceAll("_", " "));
  return parts.join(" · ") || "Unavailable";
}

function renderWeather(weather) {
  const panel = $("space-weather");
  panel.hidden = !weather;
  if (!weather) return;
  const modes = {
    loading: "Loading sources", live: "Current data", partial: "Partial data",
    fallback: "Scenario fallback", scenario: "Scenario only",
  };
  panel.dataset.mode = weather.mode;
  let mode = modes[weather.mode] ?? "Weather status unavailable";
  if (weather.origin === "replay") mode = weather.mode === "live" ? "Replay data" : `Replay · ${mode}`;
  if (weather.origin === "archive") mode = "Archived observations";
  text($("weather-mode"), mode);
  text($("weather-glotec"), formatObservation(weather.glotec));
  const kp = weather.kp?.value;
  text($("weather-kp"), formatObservation(weather.kp, Number.isFinite(kp) ? kp : null));
  $("weather-drap-field").hidden = !weather.drap;
  text($("weather-drap"), formatObservation(weather.drap));
}

function readSnapshot(value) {
  if (![value.stations, value.bands, value.active, value.recent].every(Array.isArray)) throw Error("Invalid simulation snapshot");
  stations = new Map(value.stations.filter((station) => station && station.id != null)
    .map((station) => [String(station.id), station]));
  bands = new Map(value.bands.filter((band) => band && band.id != null)
    .map((band) => [String(band.id), band]));
  active.clear();
  for (const item of value.active) {
    const attempt = normalizedAttempt(item);
    if (attempt?.state === "active") active.set(attempt.id, attempt);
  }
  recent = value.recent.map(normalizedAttempt).filter((attempt) => attempt && attempt.state !== "active").slice(0, 32);
  setOptions(stationFilter, "All stations", [...stations].map(([id, station]) => new Option(station.name ?? id, id)));
  setOptions(bandFilter, "All bands", [...bands.keys()].map((id) => new Option(bandName(id), id)));
  stationConsole?.configure([...stations.values()], value.bandwidth_hz ?? 500);
  if (inAntennaLab) $("station-console").hidden = true;
  stationConsole?.selectStation(stationFilter.value);
  antennaCatalog = value.antennas?.catalog ?? null;
  antennaStations = new Map((value.antennas?.stations ?? []).map(item => [item.station, item]));
  $("map-pattern-control").hidden = inAntennaLab || antennaStations.size === 0;
  antennaConsole?.configure(value, stations, stationFilter.value);
  waveform.select(stationFilter.value, true);
  text($("station-count"), `${stations.size} stations · ${bands.size} bands`);
  text($("model-name"), `${value.channel ?? "GNU Radio"}${Number.isFinite(value.sample_rate_hz) ? ` · ${number.format(value.sample_rate_hz)} samples/s` : ""}`);
  text($("seed"), value.seed == null ? "Seed —" : `Seed ${value.seed}`);
  applyStatus(value);
  synchronized = true;
  learning?.onSnapshot(value);
  antennaLab?.onSnapshot(value);
  globe?.setStations();
  globe?.drawNetworkAntennas();
  renderStatus();
  renderTraffic();
}

function readAttempt(value) {
  const attempt = normalizedAttempt(value.attempt);
  if (!attempt) throw Error("Invalid simulation attempt");
  applyStatus(value);
  if (!Number.isFinite(value.elapsed_s) && Number.isFinite(attempt.simulated_at_s)) {
    elapsed = Math.max(elapsed, attempt.simulated_at_s);
  }
  recent = recent.filter((item) => item.id !== attempt.id);
  waveform.notify(attempt);
  if (inAntennaLab && attempt.state === "active") drawAttemptAntenna(attempt);
  if (attempt.state === "active") {
    active.set(attempt.id, attempt);
  } else {
    active.delete(attempt.id);
    recent.unshift(attempt);
    recent = recent.slice(0, 32);
  }
  renderStatus();
  renderTraffic();
}

function eventHandler(handler) {
  return (event) => {
    try {
      handler(JSON.parse(event.data));
    } catch (problem) {
      setNotice(`Unable to read the live feed. ${problem.message}`);
    }
  };
}

async function startBrowserMode(bandwidth = Number(modeControl.value)) {
  modeControl.value = String(bandwidth);
  const previous = browserTransport;
  browserTransport = null;
  modeControl.disabled = true;
  waveform.select("");
  stationConsole.configure([], bandwidth);
  antennaCatalog = null;
  antennaStations.clear();
  globe?.drawNetworkAntennas();
  antennaConsole?.configure(null, new Map());
  active.clear();
  recent = [];
  stats = {};
  elapsed = 0;
  selected = null;
  running = false;
  error = null;
  synchronized = false;
  transport = "connecting";
  bandFilter.value = "";
  text($("seed"), "Seed —");
  text($("model-name"), `Loading BW${bandwidth}`);
  applyStatus({ weather: null, forecast: null });
  renderStatus();
  renderTraffic();
  try {
    if (previous) await previous.close();
    const handlers = { snapshot: readSnapshot, attempt: readAttempt, status: applyStatus };
    browserTransport = createBrowserTransport((kind, value) => {
      try {
        if (kind === "station-message") {
          stationConsole.receive(value);
          learning?.onMessage(value);
          return;
        }
        if (kind === "station-radio") {
          stationConsole.receiveRadio(value);
          learning?.onRadio(value);
          return;
        }
        if (kind === "station-antenna") {
          antennaConsole?.update(value);
          if (!value.error && antennaStations.has(value.station)) {
            antennaStations.set(value.station, { ...antennaStations.get(value.station), ...value });
            globe?.drawNetworkAntennas(value.station);
          }
          return;
        }
        if (kind === "antenna-trial") {
          antennaLab?.onTrial(value);
          return;
        }
        transport = "open";
        if (kind === "status" && value.error) {
          synchronized = true;
          learning?.onError(value.error);
          antennaLab?.onError(value.error);
        }
        handlers[kind]?.(value);
        renderStatus();
      } catch (problem) {
        setNotice(`Unable to read the browser simulation. ${problem.message}`);
      }
    }, bandwidth, { scenarioId: inAntennaLab ? "antenna-lab" : learning?.scenarioId, antennas: antennaPreset });
  } catch (problem) {
    transport = "open";
    synchronized = true;
    applyStatus({ running: false, error: problem.message });
    learning?.onError(problem.message);
    antennaLab?.onError(problem.message);
    renderStatus();
  } finally {
    modeControl.disabled = Boolean(learning) || inAntennaLab;
  }
}

function connect() {
  if (inBrowser) {
    text($("simulation-description"), "This browser runs the station network and Linkwise modems while this page is open.");
    $("browser-learning").hidden = false;
    $("browser-controls").hidden = inAntennaLab;
    const preset = $("antenna-preset-link");
    preset.hidden = Boolean(learning) || inAntennaLab;
    if (antennaPreset) {
      preset.href = "./";
      text(preset, "Standard simulation");
    }
    runControl.addEventListener("click", () => browserTransport?.setRunning(!running));
    modeControl.addEventListener("change", () => { void startBrowserMode(); });
    void startBrowserMode(inAntennaLab ? 500 : learning?.initialBandwidth);
    return;
  }
  if (!window.EventSource) {
    setNotice("This browser does not support the live event stream.");
    return;
  }
  const events = new EventSource(new URL("./events", location.href));
  events.addEventListener("open", () => {
    transport = "open";
    renderStatus();
  });
  events.addEventListener("error", () => {
    transport = "reconnect";
    synchronized = false;
    renderStatus();
  });
  events.addEventListener("snapshot", eventHandler(readSnapshot));
  events.addEventListener("attempt", eventHandler(readAttempt));
  events.addEventListener("status", eventHandler((value) => {
    applyStatus(value);
    renderStatus();
  }));
}

function focusAttempt(id, force = false) {
  const attempt = recentTraffic().find((record) => record.id === id);
  if (!attempt) return;
  selected = !force && selected === id ? null : id;
  renderTraffic();
  if (selected) globe?.focusPath(attempt);
  if (selected) drawAttemptAntenna(attempt);
}

function changeStation() {
  selected = null;
  waveform.select(stationFilter.value);
  stationConsole?.selectStation(stationFilter.value);
  antennaConsole?.selectStation(stationFilter.value);
  if (!antennaConsole && !inAntennaLab) {
    antennaOverlay = null;
    globe?.drawAntenna(null);
  }
  renderTraffic();
  if (stationFilter.value) globe?.focusStation(stationFilter.value);
}

function makeGlobe(C) {
  window.CESIUM_BASE_URL = vendor.href;
  const viewer = new C.Viewer("earth", {
    animation: false, baseLayer: false, baseLayerPicker: false, fullscreenButton: false,
    geocoder: false, homeButton: false, infoBox: false, navigationHelpButton: false,
    sceneModePicker: false, selectionIndicator: false, timeline: false,
    requestRenderMode: true, maximumRenderTimeChange: Infinity,
  });
  const scene = viewer.scene;
  scene.backgroundColor = C.Color.fromCssColorString("#0b1725");
  scene.globe.baseColor = C.Color.fromCssColorString("#223e56");
  scene.screenSpaceCameraController.minimumZoomDistance = 350000;
  const points = scene.primitives.add(new C.PointPrimitiveCollection());
  const lines = scene.primitives.add(new C.PolylineCollection());
  const bearings = scene.primitives.add(new C.PolylineCollection());
  const networkAntennas = scene.primitives.add(new C.PolylineCollection());
  const networkPatterns = new Map();
  const markers = new Map();
  const paths = new Map();
  const stationColor = C.Color.fromCssColorString("#d0ecff");
  const busyColor = C.Color.fromCssColorString("#f9c873");
  const colors = {
    active: busyColor,
    delivered: C.Color.fromCssColorString("#71e9c1"),
    failed: C.Color.fromCssColorString("#ff9b89"),
  };
  const materials = {
    active: C.Material.fromType("Color", { color: colors.active }),
    delivered: C.Material.fromType("PolylineArrow", { color: colors.delivered.withAlpha(0.9) }),
    failed: C.Material.fromType("PolylineDash", { color: colors.failed.withAlpha(0.76), dashLength: 13 }),
  };
  const antennaMaterials = {
    heading: C.Material.fromType("PolylineArrow", { color: C.Color.fromCssColorString("#b9a1ff") }),
    peer: C.Material.fromType("PolylineDash", { color: C.Color.fromCssColorString("#f5da96"), dashLength: 12 }),
    tx: C.Material.fromType("Color", { color: C.Color.fromCssColorString("#b9a1ff") }),
    rx: C.Material.fromType("Color", { color: C.Color.fromCssColorString("#7de8d7") }),
    networkTx: C.Material.fromType("Color", { color: C.Color.fromCssColorString("#b9a1ff").withAlpha(.7) }),
    networkRx: C.Material.fromType("Color", { color: C.Color.fromCssColorString("#7de8d7").withAlpha(.67) }),
  };

  function position(station, height = 0) {
    return station && Number.isFinite(station.lat) && Number.isFinite(station.lon)
      ? C.Cartesian3.fromDegrees(station.lon, station.lat, height) : null;
  }

  function antennaProfile(id) {
    return antennaCatalog?.profiles.find(profile => profile.id === id);
  }

  function antennaBand() {
    return bandFilter.value || (bands.has("20m") ? "20m" : bands.keys().next().value);
  }

  function patternPositions(station, profile, band, elevation, heading, radius, skip) {
    if (!station || !profile?.bands[band] || !Number.isFinite(heading) || !Number.isFinite(elevation)) return [];
    const cut = gainCut(profile, band, elevation, heading);
    const peak = Math.max(...cut.map(point => point.gain_dbi));
    return cut.filter((_, i) => i % skip === 0).map(({ gain_dbi, bearing_deg }) => position(
      pointAlongHeading(station, bearing_deg, radius * (.15 + .85 * 10 ** ((gain_dbi - peak) / 20))), 65000));
  }

  function drawNetworkAntennas(changedStation) {
    const role = mapPatterns.value;
    const summary = $("map-pattern-summary");
    if (!changedStation) {
      networkAntennas.removeAll();
      networkPatterns.clear();
    }
    if (inAntennaLab || !antennaCatalog || role === "off") {
      summary.hidden = true;
      scene.requestRender();
      return;
    }
    const band = antennaBand();
    const elevation = 20;
    const roles = role === "both" ? ["tx", "rx"] : [role];
    const stationIds = changedStation ? [changedStation] : antennaStations.keys();
    for (const id of stationIds) {
      const state = antennaStations.get(id);
      const station = stations.get(id);
      for (const side of roles) {
        const key = `${id}/${side}`;
        const prior = networkPatterns.get(key);
        const shape = antennaProfile(state?.[side + "Port"]);
        const heading = state?.[side + "Heading"] ?? (shape?.rotatable ? state?.actualAzimuth : state?.fixedAzimuth);
        const signature = `${shape?.id}/${band}/${heading}`;
        if (prior?.signature === signature) continue;
        const positions = patternPositions(station, shape, band, elevation, heading, side === "tx" ? .049 : .036, 2);
        if (!positions.length) {
          if (prior) networkAntennas.remove(prior.line);
          networkPatterns.delete(key);
          continue;
        }
        if (prior) {
          prior.line.positions = positions;
          prior.signature = signature;
        } else {
          const line = networkAntennas.add({ positions, width: 1.8,
            id: { stationId: id }, material: side === "tx" ? antennaMaterials.networkTx : antennaMaterials.networkRx });
          networkPatterns.set(key, { signature, line, side });
        }
      }
    }
    const patterns = [...networkPatterns.values()];
    const counts = roles.map(side => `${patterns.filter(entry => entry.side === side).length} ${side.toUpperCase()}`).join(" · ");
    const pigments = roles.map(side => side === "tx" ? "violet TX" : "teal RX").join(", ");
    summary.hidden = false;
    text(summary, `${bands.get(band)?.name ?? band} · ${counts} patterns across ${antennaStations.size} stations · ${elevation}° elevation · ${pigments} · relative scale`);
    scene.requestRender();
  }

  function overview() {
    viewer.camera.setView({ destination: C.Cartesian3.fromDegrees(-35, 21, 17700000) });
    scene.requestRender();
  }

  function setStations() {
    points.removeAll();
    markers.clear();
    paths.clear();
    for (const [id, station] of stations) {
      const pin = position(station, 14000);
      if (!pin) continue;
      markers.set(id, points.add({
        position: pin, pixelSize: 7, color: stationColor,
        outlineWidth: 1.5, outlineColor: C.Color.fromCssColorString("#0a1623"),
        id: { stationId: id },
      }));
    }
  }

  function arc(attempt) {
    const key = `${attempt.source}/${attempt.target}`;
    if (paths.has(key)) return paths.get(key);
    const from = stations.get(attempt.source);
    const to = stations.get(attempt.target);
    if (!position(from) || !position(to)) return null;
    const start = C.Cartographic.fromDegrees(from.lon, from.lat);
    const end = C.Cartographic.fromDegrees(to.lon, to.lat);
    const geodesic = new C.EllipsoidGeodesic(start, end);
    const distance = geodesic.surfaceDistance;
    if (!Number.isFinite(distance) || distance === 0) return null;
    const segments = Math.min(60, Math.max(16, Math.ceil(distance / 220000)));
    const rise = Math.min(720000, Math.max(42000, distance * .085));
    const result = Array.from({ length: segments + 1 }, (_, i) => {
      const t = i / segments;
      const point = geodesic.interpolateUsingFraction(t);
      return C.Cartesian3.fromRadians(point.longitude, point.latitude, 14000 + rise * Math.sin(Math.PI * t));
    });
    if (paths.size >= 256) paths.delete(paths.keys().next().value);
    paths.set(key, result);
    return result;
  }

  function draw(attempts) {
    const current = attempts.filter((attempt) => attempt.state === "active");
    const recentPaths = attempts.filter((attempt) => attempt.state !== "active").slice(0, 22);
    const chosen = attempts.find((attempt) => attempt.id === selected);
    const display = new Map();
    for (const attempt of [...recentPaths.reverse(), ...current, ...(chosen ? [chosen] : [])]) {
      display.set([attempt.source, attempt.target].sort().join("/"), attempt);
    }
    lines.removeAll();
    for (const attempt of display.values()) {
      const geometry = arc(attempt);
      if (!geometry) continue;
      lines.add({
        positions: geometry,
        material: materials[attempt.state],
        width: attempt.id === selected ? 5 : attempt.state === "active" ? 3.4 : 2.5,
        id: { attemptId: attempt.id },
      });
    }
    const busy = new Set(current.flatMap(({ source, target }) => [source, target]));
    for (const [id, point] of markers) {
      const highlighted = id === stationFilter.value || id === chosen?.source || id === chosen?.target;
      point.pixelSize = highlighted ? 12 : busy.has(id) ? 10 : 7;
      point.color = busy.has(id) ? busyColor : stationColor;
    }
    scene.requestRender();
  }

  function drawAntenna(view) {
    bearings.removeAll();
    const label = $("map-antenna");
    label.hidden = !view;
    if (view) {
      const { source, peer, heading, bearing, rxHeading } = view;
      const addBearing = (station, angle, material, length, width) => {
        if (!position(station) || !Number.isFinite(angle)) return;
        const positions = Array.from({ length: 9 }, (_, index) =>
          position(pointAlongHeading(station, angle, length * index / 8), 65000));
        bearings.add({ positions, material, width });
      };
      const tx = antennaProfile(view.txProfile);
      const rx = antennaProfile(view.rxProfile);
      const band = view.band || antennaBand();
      const elevation = view.elevation ?? (peer ? pathGeometry(source, peer).elevation_deg : tx?.bands[band]?.elevation_deg);
      const addPattern = (station, shape, azimuth, material) => {
        const positions = patternPositions(station, shape, band, elevation, azimuth, .123, 1);
        if (!positions.length) return false;
        bearings.add({ positions, material, width: 2.4 });
        return true;
      };
      const pattern = addPattern(source, tx, heading, antennaMaterials.tx);
      const receivePattern = addPattern(peer, rx, rxHeading, antennaMaterials.rx);
      addBearing(source, heading, antennaMaterials.heading, .13, 6);
      addBearing(source, bearing, antennaMaterials.peer, .18, 3);
      addBearing(peer, rxHeading, antennaMaterials.heading, .09, 4);
      const parts = [`${stationName(source.id)} heading ${headingLabel(heading)}`];
      if (peer) parts.push(`${stationName(peer.id)} bearing ${headingLabel(bearing)}`);
      if (Number.isFinite(rxHeading)) parts.push(`RX heading ${headingLabel(rxHeading)}`);
      if (pattern) parts.push(`${bands.get(band)?.name ?? band} gain contour at ${number.format(elevation)}° elevation, relative scale`);
      if (receivePattern) parts.push("TX violet · RX teal");
      text(label, parts.join(" · "));
    }
    scene.requestRender();
  }

  function focusPath(attempt) {
    const geometry = arc(attempt);
    if (!geometry) return;
    const sphere = C.BoundingSphere.fromPoints(geometry);
    viewer.camera.flyToBoundingSphere(sphere, {
      duration: reducedMotion.matches ? 0 : .65,
      offset: new C.HeadingPitchRange(0, -Math.PI / 2, Math.max(4000000, sphere.radius * 2.8)),
    });
  }

  function focusStation(id) {
    const destination = position(stations.get(id), 6800000);
    if (destination) viewer.camera.flyTo({ destination, duration: reducedMotion.matches ? 0 : .65 });
  }

  viewer.screenSpaceEventHandler.setInputAction(({ position: screenPosition }) => {
    let picked = scene.pick(screenPosition)?.id;
    if (touchPointer.matches && !picked?.stationId) {
      const nearbyStation = scene.drillPick(screenPosition, 8, 19, 19)
        .find(({ id }) => id?.stationId);
      if (nearbyStation) picked = nearbyStation.id;
    }
    if (picked?.stationId) {
      stationFilter.value = picked.stationId;
      changeStation();
    } else if (picked?.attemptId) {
      focusAttempt(picked.attemptId);
    }
  }, C.ScreenSpaceEventType.LEFT_CLICK);
  $("zoom-in").addEventListener("click", () => viewer.camera.zoomIn(viewer.camera.positionCartographic.height / 3));
  $("zoom-out").addEventListener("click", () => viewer.camera.zoomOut(viewer.camera.positionCartographic.height / 3));
  $("overview").addEventListener("click", () => {
    stationFilter.value = "";
    changeStation();
    overview();
  });
  overview();
  return { viewer, setStations, draw, drawAntenna, drawNetworkAntennas, focusPath, focusStation };
}

async function setupMap() {
  const message = $("map-message");
  try {
    if (!window.Cesium) throw Error("Cesium is unavailable");
    globe = makeGlobe(window.Cesium);
  } catch {
    $("earth").replaceChildren();
    text(message, "The globe requires local Cesium assets and WebGL. The traffic list remains available.");
    return;
  }
  const C = window.Cesium;
  const scene = globe.viewer.scene;
  scene.canvas.addEventListener("webglcontextlost", () => {
    message.hidden = false;
    text(message, "The globe lost its graphics context. Reload to restore the map.");
  });
  document.querySelector(".map-controls").hidden = false;
  globe.setStations();
  globe.drawAntenna(antennaOverlay);
  globe.drawNetworkAntennas();
  renderTraffic();
  const attribution = $("basemap-source");
  try {
    const provider = await C.SingleTileImageryProvider.fromUrl(new URL("blue-marble-clear.jpg", vendor).href, {
      credit: "NASA Earth Observatory · Blue Marble Next Generation",
    });
    globe.viewer.imageryLayers.addImageryProvider(provider);
    text(attribution, "NASA Blue Marble · Cloud-free · Aug 2004");
  } catch {
    try {
      const provider = await C.TileMapServiceImageryProvider.fromUrl(new URL("Assets/Textures/NaturalEarthII/", vendor).href, {
        credit: "Natural Earth",
      });
      globe.viewer.imageryLayers.addImageryProvider(provider);
      text(attribution, "Natural Earth basemap");
    } catch {
      text(message, "Basemap unavailable. Station markers and calls are still visible.");
      return;
    }
  }
  attribution.hidden = false;
  scene.requestRender();
  message.hidden = true;
}

stationFilter.addEventListener("change", changeStation);
bandFilter.addEventListener("change", () => {
  selected = null;
  renderTraffic();
  globe?.drawAntenna(antennaOverlay);
  globe?.drawNetworkAntennas();
});
mapPatterns.addEventListener("change", () => globe?.drawNetworkAntennas());
connect();
setupMap();
