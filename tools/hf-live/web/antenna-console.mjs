import { pathGeometry } from "./antenna-model.mjs";

const radians = Math.PI / 180;
const degrees = 1 / radians;
const format = new Intl.NumberFormat("en-US", { maximumFractionDigits: 1 });

export function headingLabel(value) {
  return Number.isFinite(value) ? format.format(value) + "° true" : "—";
}

export function antennaReadout(value, name) {
  if (!value?.tx || !value.rx) return null;
  if (Object.hasOwn(value, "eirpW")) return value;
  const mount = (endpoint, bearingDeg) => ({
    profileId: endpoint.profile_id, label: name(endpoint.profile_id),
    headingDeg: endpoint.heading_deg, bearingDeg, gainDb: endpoint.gain_dbi,
    feedlineLossDb: endpoint.feedline_loss_db,
  });
  return {
    tx: mount(value.tx, value.tx_bearing_deg), rx: mount(value.rx, value.rx_bearing_deg),
    elevationDeg: value.elevation_deg, conductedW: value.conducted_power_w, eirpW: value.eirp_w,
  };
}

function peerBearing(from, to) {
  return from && to && from.id !== to.id ? pathGeometry(from, to).tx_bearing_deg : null;
}

export function pointAlongHeading(from, heading, arc) {
  const lat = from.lat * radians;
  const lon = from.lon * radians;
  const direction = heading * radians;
  const endLat = Math.asin(Math.sin(lat) * Math.cos(arc)
    + Math.cos(lat) * Math.sin(arc) * Math.cos(direction));
  const endLon = lon + Math.atan2(Math.sin(direction) * Math.sin(arc) * Math.cos(lat),
    Math.cos(arc) - Math.sin(lat) * Math.sin(endLat));
  return { lat: endLat * degrees, lon: endLon * degrees };
}

export function createAntennaConsole({ transport, onStation, onOverlay }) {
  const field = name => document.getElementById("antenna-" + name);
  const section = field("console");
  const form = field("form");
  const stationSelect = field("station");
  const peerSelect = field("peer");
  const txSelect = field("tx-port");
  const rxSelect = field("rx-port");
  const heading = field("heading");
  const auto = field("auto");
  const aim = field("aim");
  const save = field("save");
  const notice = field("notice");
  let stationMap = new Map();
  let antennaMap = new Map();
  let submitted = null;

  function current() {
    return antennaMap.get(stationSelect.value);
  }

  function portsFor(state, role) {
    return (state?.ports ?? []).filter(port => role === "rx" || port.role === "tx-rx");
  }

  function fillPorts(select, options, selected) {
    select.replaceChildren(...options.map(port => {
      const label = port.name + (port.bands.length ? " · " + port.bands.join(", ") : "");
      return new Option(label, port.id);
    }));
    select.value = selected ?? "";
  }

  function canAim() {
    const state = current();
    return [txSelect.value, rxSelect.value].some(id => {
      return state?.ports.some(port => port.id === id && port.rotatable);
    });
  }

  function readback() {
    const state = current();
    const source = stationMap.get(stationSelect.value);
    const peer = stationMap.get(peerSelect.value);
    const bearing = peerBearing(source, peer);
    field("current").textContent = headingLabel(state?.actualAzimuth);
    const motion = { rotating: "rotating", settling: "settling", switching: "switching", queued: "queued" }[state?.state];
    field("target").textContent = headingLabel(state?.targetAzimuth) + (motion ? " · " + motion : "");
    field("bearing").textContent = headingLabel(bearing);
    const rotatable = canAim();
    heading.disabled = !rotatable;
    heading.required = rotatable;
    auto.disabled = !state;
    aim.disabled = !rotatable || !Number.isFinite(bearing);
    save.disabled = !state || !txSelect.value || !rxSelect.value;
    const activeTx = state?.ports.find(port => port.id === state.txPort);
    const activeHeading = activeTx?.rotatable ? state.actualAzimuth : state?.fixedAzimuth;
    onOverlay(source ? { source, peer, bearing, heading: activeHeading, txProfile: state?.txPort } : null);
  }

  function fillControls() {
    const state = current();
    fillPorts(txSelect, portsFor(state, "tx"), state?.targetTxPort ?? state?.txPort);
    fillPorts(rxSelect, portsFor(state, "rx"), state?.targetRxPort ?? state?.rxPort);
    heading.value = Number.isFinite(state?.targetAzimuth) ? String(Math.floor(state.targetAzimuth * 10) / 10) : "0";
    auto.checked = Boolean(state?.autoAim);
    readback();
  }

  function selectStation(id) {
    if (!stationMap.has(id) || stationSelect.value === id) return;
    stationSelect.value = id;
    refillPeers();
    notice.textContent = "";
    fillControls();
  }

  function refillPeers() {
    const previous = peerSelect.value;
    peerSelect.replaceChildren(new Option("Select a peer", ""),
      ...[...stationMap.values()].filter(item => item.id !== stationSelect.value)
        .map(item => new Option(item.name ?? item.id, item.id)));
    if (previous !== stationSelect.value && stationMap.has(previous)) peerSelect.value = previous;
  }

  stationSelect.addEventListener("change", () => {
    refillPeers();
    notice.textContent = "";
    fillControls();
    onStation(stationSelect.value);
  });
  peerSelect.addEventListener("change", readback);
  txSelect.addEventListener("change", readback);
  rxSelect.addEventListener("change", readback);
  aim.addEventListener("click", () => {
    const bearing = peerBearing(stationMap.get(stationSelect.value), stationMap.get(peerSelect.value));
    if (!Number.isFinite(bearing)) return;
    heading.value = String((Math.round(bearing * 10) % 3600) / 10);
    form.requestSubmit();
  });
  form.addEventListener("submit", event => {
    event.preventDefault();
    const worker = transport();
    if (!worker || save.disabled || !form.reportValidity()) return;
    const station = stationSelect.value;
    const request = { station, txPort: txSelect.value, rxPort: rxSelect.value, autoAim: auto.checked };
    if (canAim()) request.azimuth = heading.valueAsNumber;
    try {
      worker.setAntenna(request);
      submitted = station;
      notice.textContent = "Applying antenna settings…";
    } catch (error) {
      notice.textContent = error.message;
    }
  });

  return {
    configure(snapshot, stations, selected) {
      const previous = selected || stationSelect.value;
      stationMap = stations;
      const data = snapshot?.antennas;
      const entries = data?.stations ?? [];
      antennaMap = new Map(entries.map(item => [item.station, item]));
      submitted = null;
      section.hidden = !data?.enabled || !entries.length;
      if (section.hidden) {
        onOverlay(null);
        return;
      }
      stationSelect.replaceChildren(...entries.map(item =>
        new Option(stations.get(item.station)?.name ?? item.station, item.station)));
      if (antennaMap.has(previous)) stationSelect.value = previous;
      refillPeers();
      notice.textContent = "";
      fillControls();
    },
    selectStation,
    update(state) {
      if (!antennaMap.has(state.station)) return;
      if (state.error) {
        if (submitted === state.station) submitted = null;
        if (stationSelect.value === state.station) notice.textContent = state.error;
        return;
      }
      antennaMap.set(state.station, { ...antennaMap.get(state.station), ...state });
      if (stationSelect.value !== state.station) return;
      if (submitted === state.station) {
        submitted = null;
        fillControls();
        notice.textContent = "Antenna settings accepted.";
      } else {
        readback();
      }
    },
  };
}
