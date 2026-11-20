import { solve, densityFraction } from "./ray.mjs";
const C = window.Cesium,
  $ = (id) => document.getElementById(id);
const mint = C.Color.fromCssColorString("#70edcb"),
  orange = C.Color.fromCssColorString("#ff997c");
let contactsLayer, materials, contactsBySlot;
const groundPaths = new Map();
let viewer,
  dots,
  replay,
  frames,
  timer,
  slot,
  msg,
  circuit,
  run,
  eventSlots,
  byMessage;
let forecasts,
  blocked,
  stationId,
  weatherLayer,
  layerKey,
  request = 0;
const rings = ["A", "B", "C", "D", "E", "F"];
const tag = (i) =>
  replay.directory_date
    ? replay.stations[i].name
    : replay.scenario === "six"
      ? rings[i]
      : "N" + String(i + 1).padStart(3, "0");
const point = (s, h = 0) => C.Cartesian3.fromDegrees(s.lon, s.lat, h);
function fail(e) {
  $("error").hidden = false;
  $("error").textContent = String(e);
}
const read = async (url) => {
  const r = await fetch(url);
  if (!r.ok) throw Error(url + " " + r.status);
  return r.json();
};
function selectOptions(id, options, current) {
  const select = $(id);
  select.replaceChildren(
    ...options.map(([value, text]) => new Option(text, value)),
  );
  select.value = current;
}
const cachedGrids = new Map(),
  pendingGrids = new Set();
function currentFrame(ms) {
  const meta = frames.find((f) => ms >= f.ms && ms < f.ms + 600000);
  if (!meta) return null;
  if (cachedGrids.has(meta.file)) return cachedGrids.get(meta.file);
  if (!pendingGrids.has(meta.file)) {
    pendingGrids.add(meta.file);
    fetch(meta.file)
      .then(async (r) => {
        if (!r.ok) throw Error("Missing weather grid " + meta.file);
        const stream = r.body.pipeThrough(new DecompressionStream("gzip"));
        const cells = await new Response(stream).json();
        if (cachedGrids.size >= 8)
          cachedGrids.delete(cachedGrids.keys().next().value);
        cachedGrids.set(meta.file, { ...meta, cells });
        const viewing =
          Date.parse(replay.utc) + slot * replay.seconds_per_slot * 1000;
        if (viewing >= meta.ms && viewing < meta.ms + 600000) draw();
      })
      .catch(fail)
      .finally(() => pendingGrids.delete(meta.file));
  }
  return null;
}
function cell(frame, lat, lon) {
  if (!frame) return null;
  const x = Math.floor(((((lon + 180) % 360) + 360) % 360) / 5),
    y = Math.max(0, Math.min(71, Math.floor((lat + 90) / 2.5)));
  return frame.cells[y * 72 + x];
}
function rgb(t) {
  const colors = [
    [33, 64, 129],
    [34, 143, 159],
    [238, 213, 114],
  ];
  t = Math.max(0, Math.min(1, t)) * 2;
  const k = Math.min(1, Math.floor(t));
  return colors[k].map((v, i) =>
    Math.round(v + (colors[k + 1][i] - v) * (t - k)),
  );
}
function weatherMap(frame, ms) {
  const loading =
    !frame && frames.some((f) => ms >= f.ms && ms < f.ms + 600000);
  const kind = $("layer").value,
    key = (frame?.utc || (loading ? "loading" : "gap")) + kind;
  if (key === layerKey) return;
  layerKey = key;
  if (weatherLayer) viewer.imageryLayers.remove(weatherLayer);
  weatherLayer = null;
  $("scale").textContent = {
    tec: "0 — 60+ TECU",
    quality: "0 — 5 observation flag",
    height: "150 — 450 km",
    off: "",
  }[kind];
  document.querySelector(".ramp").hidden = kind === "off";
  $("weather").textContent = frame
    ? "NOAA GloTEC · " + frame.utc.slice(11, 16) + " UTC · 10 min interval"
    : loading
      ? "Loading archived grid…"
      : "Archive gap at this time";
  if (!frame || kind === "off") return;
  const canvas = document.createElement("canvas");
  canvas.width = 72;
  canvas.height = 72;
  const ctx = canvas.getContext("2d"),
    pixels = ctx.createImageData(72, 72);
  frame.cells.forEach((c, i) => {
    if (!c) return;
    const t =
      kind === "tec"
        ? c[0] / 60
        : kind === "quality"
          ? c[3] / 5
          : (c[1] - 150) / 300;
    pixels.data.set(
      [...rgb(t), 255],
      ((71 - Math.floor(i / 72)) * 72 + (i % 72)) * 4,
    );
  });
  ctx.putImageData(pixels, 0, 0);
  weatherLayer = viewer.imageryLayers.addImageryProvider(
    new C.SingleTileImageryProvider({
      url: canvas.toDataURL(),
      tileWidth: 72,
      tileHeight: 72,
    }),
  );
  weatherLayer.alpha = 0.4;
}
function geodesic(a, b) {
  return new C.EllipsoidGeodesic(
    C.Cartographic.fromDegrees(a.lon, a.lat),
    C.Cartographic.fromDegrees(b.lon, b.lat),
  );
}
function stop() {
  clearInterval(timer);
  timer = null;
  $("play").textContent = "▶ Play";
}
function home() {
  const six = replay.stations.length === 6;
  viewer.camera.setView({
    destination: C.Cartesian3.fromDegrees(
      replay.directory_date ? (six ? 3 : -90) : six ? -30 : -154,
      six ? 46 : 17,
      six ? 8800000 : 20500000,
    ),
  });
}
function chooseRun() {
  run = replay.runs[+$("policy").value + ($("gate").checked ? 2 : 0)];
  byMessage = replay.offers.map(() => []);
  eventSlots = Array.from({ length: replay.slots + 1 }, () => []);
  for (const e of run.events) {
    eventSlots[e[0]].push(e);
    byMessage[e[1]].push(e);
  }
  chooseMessage(false);
}
function chooseMessage(reset = true) {
  msg = +$("message").value;
  const events = byMessage[msg].filter((e) => e[2] !== null);
  const ids = [...new Set(events.map((e) => e[2]))];
  const source = replay.offers[msg].source;
  if (!ids.length) {
    const first = replay.contacts.findIndex((c) => c[1] === source);
    if (first >= 0) ids.push(first);
  }
  const selected =
    ids.find((id) => replay.contacts[id][0] >= slot) ?? ids.at(-1);
  if (reset || !ids.includes(circuit)) circuit = selected;
  selectOptions(
    "circuit",
    ids.map((id) => {
      const [s, a, b, ch] = replay.contacts[id];
      return [
        id,
        tag(a) +
          " → " +
          tag(b) +
          " · slot " +
          s +
          " · " +
          (replay.frequency_hz[ch] / 1e6).toFixed(
            replay.directory_date ? 4 : 1,
          ) +
          " MHz",
      ];
    }),
    circuit,
  );
  const list = $("events");
  list.replaceChildren();
  for (const [time, id, link, , outcome] of byMessage[msg]) {
    const button = document.createElement("button"),
      c = link == null ? null : replay.contacts[link];
    button.textContent =
      "Slot " +
      time +
      " · " +
      (c ? tag(c[1]) + " → " + tag(c[2]) + " · " : "") +
      outcome;
    button.onclick = () => {
      stop();
      $("time").value = time;
      if (c) {
        circuit = link;
        $("circuit").value = link;
      }
      draw();
    };
    list.append(button);
  }
  draw();
}
function addLine(points, color, width, dashed, id) {
  viewer.entities.add({
    polyline: {
      positions: points,
      width,
      arcType: C.ArcType.NONE,
      material: dashed
        ? new C.PolylineDashMaterialProperty({ color, dashLength: 12 })
        : color,
    },
    properties: id,
  });
}
function ground(a, b) {
  const g = geodesic(a, b);
  return Array.from({ length: 33 }, (_, i) => {
    const p = g.interpolateUsingFraction(i / 32);
    return C.Cartesian3.fromRadians(p.longitude, p.latitude, 6000);
  });
}
function calcCircuit(frame, ch) {
  if (circuit == null) return null;
  const mhz = replay.frequency_hz[ch] / 1e6;
  const [, ai, bi] = replay.contacts[circuit],
    a = replay.stations[ai],
    b = replay.stations[bi];
  const g = geodesic(a, b),
    km = g.surfaceDistance / 1000,
    count = Math.max(1, Math.ceil(km / 2800));
  const mid = g.interpolateUsingFraction(0.5);
  const sample = cell(
    frame,
    C.Math.toDegrees(mid.latitude),
    C.Math.toDegrees(mid.longitude),
  );
  const heights = [],
    profile = [],
    points = [],
    rayProfile = [],
    rayPoints = [];
  let connected = true;
  let ceiling = Infinity,
    slant = 0,
    complete = true;
  for (let j = 0; j < count; j++) {
    const m = g.interpolateUsingFraction((j + 0.5) / count);
    const v = cell(
      frame,
      C.Math.toDegrees(m.latitude),
      C.Math.toDegrees(m.longitude),
    );
    const h = v?.[1] || 300;
    heights.push(h);
    complete &&= !!v && h > 0 && v[2] > 0;
    const angle = km / (count * 2 * 6371),
      radial = 6371 + h - 6371 * Math.cos(angle);
    const leg = Math.hypot(radial, 6371 * Math.sin(angle));
    slant += 2 * leg;
    if (v)
      ceiling = Math.min(
        ceiling,
        (((8.98 * Math.sqrt(v[2] * 1e9)) / 1e6) * leg) / radial,
      );
    if ($("physics").value === "ray") {
      const ray = v && v[2] > 0 ? solve(km / count, h, v[2] * 1e9, mhz) : null;
      if (!ray) connected = false;
      else
        for (const [distance, height] of ray.path) {
          const x = (j + distance / ray.distance) / count,
            p = g.interpolateUsingFraction(x);
          rayProfile.push([x, height]);
          rayPoints.push(
            C.Cartesian3.fromRadians(
              p.longitude,
              p.latitude,
              height * 1000 + 100,
            ),
          );
        }
    }
    for (let k = 0; k <= 24; k++) {
      const t = k / 24,
        p = g.interpolateUsingFraction((j + t) / count),
        z = 4 * t * (1 - t) * h;
      points.push(
        C.Cartesian3.fromRadians(p.longitude, p.latitude, z * 1000 + 100),
      );
      profile.push([(j + t) / count, z]);
    }
  }
  const family = replay.scenario === "six" ? "six" : "global";
  const hour = new Date(
    Date.parse(replay.utc) + slot * replay.seconds_per_slot * 1000,
  ).getUTCHours();
  const prediction = replay.directory_date
    ? undefined
    : forecasts[[family, ai, bi, hour, ch].join("/")];
  const f2 = [];
  for (let k = 0; k <= 40; k++) {
    const p = g.interpolateUsingFraction(k / 40);
    f2.push(
      cell(
        frame,
        C.Math.toDegrees(p.latitude),
        C.Math.toDegrees(p.longitude),
      )?.[1],
    );
  }
  const isRay = $("physics").value === "ray";
  return {
    prediction,
    f2,
    points: isRay ? (connected ? rayPoints : []) : points,
    profile: isRay ? (connected ? rayProfile : []) : profile,
    isRay,
    connected,
    km,
    count,
    sample,
    heights,
    ceiling: complete ? ceiling : null,
    loss: 32.45 + 20 * Math.log10(slant * mhz),
    mhz,
    mid,
  };
}
function inspector(model, frame) {
  const canvas = $("section"),
    ctx = canvas.getContext("2d");
  ctx.clearRect(0, 0, 600, 156);
  if (!model) {
    $("metrics").replaceChildren();
    $("model").textContent = "No scheduled circuit.";
    return;
  }
  const items = [
    [model.km.toFixed(0) + " km", "Ground distance"],
    [model.mhz.toFixed(2) + " MHz", "Channel"],
    [
      model.ceiling === null ? "No grid" : model.ceiling.toFixed(1) + " MHz",
      "F2 secant estimate",
    ],
    [model.loss.toFixed(1) + " dB", "Free-space spreading"],
  ];
  const pred = model.prediction;
  if (pred)
    items.push(
      [Math.round(pred.REL * 100) + "%", "VOACAP reliability"],
      [pred.SNR + " dB·Hz", "VOACAP median S/N₀"],
      [pred["S DBW"] + " dBW", "Received signal"],
      [pred.MODE, "Dominant mode"],
    );
  $("metrics").replaceChildren(
    ...items.map(([v, k]) => {
      const el = document.createElement("div"),
        b = document.createElement("b"),
        s = document.createElement("span");
      b.textContent = v;
      s.textContent = k;
      el.append(b, s);
      return el;
    }),
  );
  const top = Math.max(450, ...model.heights);
  if (frame) {
    for (let i = 0; i < model.f2.length; i++) {
      const peak = model.f2[i];
      if (!peak) continue;
      for (let j = 0; j < 22; j++) {
        const density = densityFraction(((j + 0.5) * top) / 22, peak);
        ctx.fillStyle = "rgba(70,110,181," + density * 0.58 + ")";
        ctx.fillRect(12 + (i * 576) / 41, 128 - (j + 1) * 5, 576 / 41 + 1, 5);
      }
    }
  }
  ctx.strokeStyle = "#4c6272";
  ctx.beginPath();
  ctx.moveTo(10, 128);
  ctx.lineTo(590, 128);
  ctx.stroke();
  ctx.strokeStyle = "#70edcb";
  ctx.lineWidth = 3;
  ctx.beginPath();
  model.profile.forEach(([x, y], i) =>
    ctx[i ? "lineTo" : "moveTo"](12 + 576 * x, 128 - (110 * y) / top),
  );
  ctx.stroke();
  ctx.beginPath();
  ctx.strokeStyle = "#ffca77";
  ctx.setLineDash([6, 5]);
  model.f2.forEach((h, i) => {
    if (h != null)
      ctx[i && model.f2[i - 1] != null ? "lineTo" : "moveTo"](
        12 + (576 * i) / 40,
        128 - (110 * h) / top,
      );
  });
  ctx.stroke();
  ctx.setLineDash([]);
  ctx.fillStyle = "#c3d3d9";
  ctx.font = "21px sans-serif";
  ctx.fillText("0 km", 12, 151);
  ctx.fillText(
    model.isRay
      ? model.connected
        ? "Refracted F2 ray"
        : "No connecting modeled ray"
      : "Schematic skywave",
    12,
    24,
  );
  $("model").textContent =
    (model.isRay
      ? model.connected
        ? "Connecting ray. "
        : "No connecting modeled ray. "
      : "") +
    (frame
      ? "GloTEC peak height sampled per hop. "
      : "300 km assumed peak height. ") +
    (model.sample
      ? "Midpoint " +
        model.sample[0] +
        " TECU · coverage " +
        model.sample[3] +
        ". "
      : "") +
    "Gold: F2 peak. Blue: modeled relative electron density (Chapman, 50 km scale)." +
    (pred
      ? " VOACAP monthly forecast, 100 W isotropic, SSN 83 assumed."
      : " No archived VOACAP forecast for this circuit.");
}
function draw() {
  if (!replay || !run) return;
  if (replay.directory_date) {
    const c = replay.contacts[circuit];
    const listed = c ? replay.stations[c[2]].channels_hz : [];
    for (const option of $("band").options) {
      const index = Number(option.value);
      option.disabled =
        index >= 0 && !listed.includes(replay.frequency_hz[index]);
    }
    if ($("band").selectedOptions[0]?.disabled) $("band").value = -1;
  }
  slot = +$("time").value;
  const ms = Date.parse(replay.utc) + slot * replay.seconds_per_slot * 1000,
    date = new Date(ms),
    frame = currentFrame(ms);
  viewer.clock.currentTime = C.JulianDate.fromDate(date);
  weatherMap(frame, ms);
  $("clock").textContent =
    date.toISOString().replace("T", " ").slice(0, 19) + " UTC";
  $("slot").textContent = "Slot " + slot + " / " + replay.slots;
  viewer.entities.removeAll();
  const events = eventSlots[slot] || [],
    offer = replay.offers[msg];
  let owner = offer.source,
    arrived = false,
    rejected = false;
  const history = [tag(owner)];
  for (const e of byMessage[msg]) {
    if (e[0] > slot) break;
    if (e[4] === "rejected") {
      rejected = true;
      continue;
    }
    if (e[0] < slot && e[4] === "transfer") {
      owner = replay.contacts[e[2]][2];
      history.push(tag(owner));
      arrived = owner === offer.destination;
    }
  }
  const active = events.filter((e) => e[2] !== null);
  $("sub").textContent =
    replay.stations.length +
    " stations · " +
    active.length +
    " calls · " +
    run.summary.delivered +
    "/" +
    run.summary.offered +
    " delivered by run end" +
    (replay.directory_date
      ? " · Winlink directory " + replay.directory_date + " · simulated traffic"
      : "");
  $("state").textContent = rejected
    ? "Rejected at source"
    : slot < offer.start
      ? "Awaiting submission"
      : arrived
        ? "Delivered at " + tag(owner)
        : slot >= offer.deadline
          ? "Expired at " + tag(owner)
          : "Queued at " + tag(owner);
  $("route").textContent =
    history.join(" → ") +
    (stationId != null
      ? "   ·   " + tag(stationId) + " / " + replay.stations[stationId].name
      : "");
  let used = 0;
  function drawContact(id, style, width) {
    const c = replay.contacts[id],
      key = c[1] + "/" + c[2];
    if (!groundPaths.has(key))
      groundPaths.set(
        key,
        ground(replay.stations[c[1]], replay.stations[c[2]]),
      );
    const line =
      used < contactsLayer.length
        ? contactsLayer.get(used)
        : contactsLayer.add();
    used++;
    line.show = true;
    line.positions = groundPaths.get(key);
    line.material = materials[style];
    line.width = width;
    line.id = { contact: id };
  }
  if ($("potential").checked) {
    for (const id of contactsBySlot[slot])
      if (!(run.forecast_gate && blocked.has(id)))
        drawContact(id, "scheduled", 1);
  }
  for (const e of active) {
    if (e[4] === "transfer") drawContact(e[2], "success", 5);
    else drawContact(e[2], "failure", 2);
  }
  for (let i = used; i < contactsLayer.length; i++)
    contactsLayer.get(i).show = false;
  const c = replay.contacts[circuit];
  const model = c
    ? calcCircuit(frame, +$("band").value < 0 ? c[3] : +$("band").value)
    : null;
  if (
    model?.points.length &&
    (!document.body.classList.contains("orbital") ||
      document.body.dataset.shot === "2")
  )
    addLine(model.points, C.Color.fromCssColorString("#ffd282"), 3, false, {
      contact: circuit,
    });
  const focus = new Set([
    owner,
    offer.destination,
    stationId,
    ...(c ? c.slice(1, 3) : []),
  ]);
  for (const i of focus) {
    if (i == null) continue;
    const s = replay.stations[i];
    viewer.entities.add({
      position: point(s, 12000),
      label: {
        text: replay.scenario === "six" ? s.name : tag(i),
        font: "bold 15px sans-serif",
        pixelOffset: new C.Cartesian2(0, -15),
        fillColor: C.Color.WHITE,
        style: C.LabelStyle.FILL_AND_OUTLINE,
        outlineWidth: 3,
      },
      properties: { station: i },
    });
  }
  if (stationId != null) {
    let queued = 0;
    replay.offers.forEach((o, id) => {
      if (slot < o.start || slot >= o.deadline) return;
      let at = o.source;
      for (const e of byMessage[id]) {
        if (e[0] > slot) break;
        if (e[4] === "rejected") {
          at = -1;
          break;
        }
        if (e[0] < slot && e[4] === "transfer") at = replay.contacts[e[2]][2];
      }
      if (at === stationId && at !== o.destination) queued++;
    });
    const calls = active.filter((e) =>
      replay.contacts[e[2]].slice(1, 3).includes(stationId),
    );
    $("stationInfo").textContent =
      tag(stationId) +
      " · " +
      queued +
      " queued · " +
      (calls.map((e) => e[4]).join(", ") || "idle");
  } else {
    $("stationInfo").textContent = "";
  }
  if (c) {
    const event = byMessage[msg].find((e) => e[2] === circuit);
    $("circuit").title = event ? event[4] : "Scheduled contact";
  }
  inspector(model, frame);
  viewer.scene.requestRender();
}
async function load() {
  const seq = ++request;
  stop();
  const name = $("scenario").value;
  const data = await read("data/" + name + ".json");
  if (seq !== request) return;
  replay = data;
  groundPaths.clear();
  // draw() reuses or hides the pool. Removing the lines would destroy their shared materials.
  contactsBySlot = Array.from({ length: data.slots + 1 }, () => []);
  data.contacts.forEach((c, i) => contactsBySlot[c[0]].push(i));
  blocked = new Set(replay.blocked_contacts);
  slot = replay.stations.length === 6 ? 6 : 1;
  $("gate").disabled = replay.runs.length < 4;
  if ($("gate").disabled) $("gate").checked = false;
  selectOptions(
    "band",
    [
      [-1, "Scheduled channel"],
      ...replay.frequency_hz.map((f, i) => [
        i,
        (f / 1e6).toFixed(replay.directory_date ? 4 : 2) + " MHz",
      ]),
    ],
    -1,
  );
  msg = 0;
  stationId = null;
  circuit = undefined;
  $("time").max = replay.slots;
  $("time").value = slot;
  $("title").textContent = replay.directory_date
    ? name === "vara-europe"
      ? "European VARA gateways"
      : "Worldwide VARA gateways"
    : name === "six"
      ? "North Atlantic"
      : "Global network";
  viewer.resolutionScale = replay.stations.length >= 1000 ? 0.68 : 1;
  dots.removeAll();
  for (const s of replay.stations)
    dots.add({
      position: point(s, 4000),
      id: { station: s.id },
      color: C.Color.WHITE,
      outlineColor: C.Color.DARKSLATEGRAY,
      outlineWidth: 1,
      pixelSize: replay.stations.length > 100 ? 4 : 6,
    });
  selectOptions(
    "message",
    replay.offers.map((o, i) => [
      i,
      "M" + (i + 1) + " · " + tag(o.source) + " → " + tag(o.destination),
    ]),
    0,
  );
  chooseRun();
  home();
}
async function init() {
  [frames, forecasts] = await Promise.all([
    read("data/weather-index.json"),
    read("data/forecast.json"),
  ]);
  for (const f of frames) f.ms = Date.parse(f.utc);
  viewer = new C.Viewer("earth", {
    baseLayer: false,
    baseLayerPicker: false,
    animation: false,
    timeline: false,
    homeButton: false,
    sceneModePicker: false,
    geocoder: false,
    navigationHelpButton: false,
    fullscreenButton: false,
    infoBox: false,
    selectionIndicator: false,
    requestRenderMode: true,
    maximumRenderTimeChange: Infinity,
  });
  const [day, night] = await Promise.all([
    C.SingleTileImageryProvider.fromUrl("imagery/terra-october.jpg", {
      credit: "NASA GIBS · Terra/Aqua MODIS / Blue Marble",
    }),
    C.SingleTileImageryProvider.fromUrl("imagery/night-context.jpg", {
      credit: "NASA Earth Observatory · Black Marble 2016",
    }),
  ]);
  viewer.imageryLayers.addImageryProvider(day);
  const lights = viewer.imageryLayers.addImageryProvider(night);
  lights.dayAlpha = 0;
  lights.nightAlpha = 1;
  lights.brightness = 1.25;
  lights.gamma = 1.12;
  viewer.scene.globe.enableLighting = true;
  viewer.scene.globe.dynamicAtmosphereLighting = false;
  viewer.scene.globe.dynamicAtmosphereLightingFromSun = true;
  viewer.scene.globe.showGroundAtmosphere = true;
  viewer.scene.globe.lightingFadeOutDistance = 5e7;
  viewer.scene.globe.lightingFadeInDistance = 6e7;
  viewer.scene.globe.vertexShadowDarkness = 0.7;
  viewer.scene.globe.nightFadeOutDistance = 1000;
  viewer.scene.globe.nightFadeInDistance = 5000;
  viewer.scene.globe.atmosphereLightIntensity = 8;
  viewer.scene.skyAtmosphere.atmosphereLightIntensity = 12;
  contactsLayer = viewer.scene.primitives.add(new C.PolylineCollection());
  materials = {
    scheduled: C.Material.fromType("Color", { color: C.Color.SLATEGRAY }),
    success: C.Material.fromType("PolylineArrow", { color: mint }),
    failure: C.Material.fromType("PolylineDash", {
      color: orange,
      dashLength: 12,
    }),
  };
  dots = viewer.scene.primitives.add(new C.PointPrimitiveCollection());
  viewer.screenSpaceEventHandler.setInputAction((e) => {
    const picked = viewer.scene.pick(e.position)?.id;
    const props =
      picked?.properties?.getValue(viewer.clock.currentTime) ?? picked;
    if (props?.station != null) {
      stationId = props.station;
      draw();
    }
    if (props?.contact != null) {
      circuit = props.contact;
      if (![...$("circuit").options].some((o) => +o.value === circuit))
        $("circuit").add(new Option("Picked circuit · " + circuit, circuit));
      $("circuit").value = circuit;
      draw();
    }
  }, C.ScreenSpaceEventType.LEFT_CLICK);
  $("scenario").onchange = () => load().catch(fail);
  $("gate").onchange = chooseRun;
  $("band").onchange = draw;
  $("physics").onchange = draw;
  $("policy").onchange = chooseRun;
  $("message").onchange = () => chooseMessage();
  $("home").onclick = home;
  $("tilt").onclick = () => {
    const c = replay.contacts[circuit];
    if (!c) return;
    const m = calcCircuit(
      currentFrame(
        Date.parse(replay.utc) + slot * replay.seconds_per_slot * 1000,
      ),
      +$("band").value < 0 ? c[3] : +$("band").value,
    );
    viewer.camera.flyToBoundingSphere(
      C.BoundingSphere.fromPoints(
        m.points.length
          ? m.points
          : ground(replay.stations[c[1]], replay.stations[c[2]]),
      ),
      {
        duration: 0.8,
        offset: new C.HeadingPitchRange(
          0,
          -0.38,
          Math.max(900000, m.km * 1450),
        ),
      },
    );
  };
  $("circuit").onchange = () => {
    circuit = +$("circuit").value;
    draw();
  };
  $("time").oninput = draw;
  $("layer").onchange = draw;
  $("potential").onchange = draw;
  $("play").onclick = () => {
    if (timer) {
      stop();
      return;
    }
    if (slot >= replay.slots) {
      $("time").value = 0;
      draw();
    }
    $("play").textContent = "Ⅱ Pause";
    timer = setInterval(
      () => {
        if (slot >= replay.slots) {
          stop();
          return;
        }
        $("time").value = slot + 1;
        draw();
      },
      1000 / +$("speed").value,
    );
  };
  $("speed").onchange = () => {
    if (timer) {
      stop();
      $("play").click();
    }
  };
  await load();
  // The orbital view accepts a WGS84 ground target and a local camera pose.
  // The same replay clock, scene and picking stay active in presentation mode.
  if (new URLSearchParams(location.search).has("orbital")) {
    const film = await import("./orbital.js");
    await film.start(
      viewer,
      () => ({ slot, replay, run, msg, circuit }),
      () => {
        const c = replay.contacts[circuit];
        if (!c) return [];
        const channel = +$("band").value;
        return (
          calcCircuit(
            currentFrame(
              Date.parse(replay.utc) + slot * replay.seconds_per_slot * 1000,
            ),
            channel < 0 ? c[3] : channel,
          )?.points || []
        );
      },
      async (name) => {
        $("scenario").value = name;
        await load();
      },
    );
  }
}
init().catch(fail);
