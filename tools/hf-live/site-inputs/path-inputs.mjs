// Path predictions and NOAA products are optional. The station layout is built in.
import { bands, stations } from "./stations.mjs";
import { readBytes, readJson, sha256, WeatherInputs } from "./weather-inputs.mjs";

const RADIUS_KM = 6371;
const STEP = Math.PI / 180;
const WEATHER_POLL_MS = 120000;
const ROW_BYTES = 10;
const ROWS = stations.length * (stations.length - 1) * bands.length * 24;
const MISSING = -(2 ** 31);
const REVISION = "cd172be56dc04b154e5d2fa91cbaa6ecf5284305";
const SITE_INDEX = new Map(stations.map((station, index) => [station.id, index]));
const BAND_INDEX = new Map(bands.map((band, index) => [band.id, index]));
const bounded = (value, low, high) => Math.max(low, Math.min(high, value));
const round = (value) => Math.round(value * 100) / 100;

function monthUtc(now) {
  return new Date(now).toISOString().slice(0, 7);
}

async function loadForecast(base, options) {
  const metadata = await readJson(new URL("forecast.json", base), 16000, options);
  if (metadata?.schema !== 1 || metadata.revision !== REVISION || !/^\d{4}-(0[1-9]|1[0-2])$/.test(metadata.month_utc)
      || metadata.assumptions?.noise_bandwidth_hz !== 3000 || !Number.isInteger(metadata.rows)
      || metadata.rows <= 0 || metadata.rows > ROWS || metadata.byte_length !== ROWS * ROW_BYTES
      || !/^[a-f0-9]{64}$/.test(metadata.sha256) || metadata.file !== `forecast-${metadata.sha256.slice(0, 16)}.bin`) {
    throw new Error("Invalid staged P.533 metadata");
  }
  const data = await readBytes(new URL(metadata.file, base), metadata.byte_length, options);
  if (data.byteLength !== metadata.byte_length || await sha256(data) !== metadata.sha256) {
    throw new Error("Invalid staged P.533 table");
  }
  return { metadata, table: new DataView(data.buffer) };
}

function siteIndex(station) {
  const index = SITE_INDEX.get(station.id);
  const expected = stations[index];
  return expected && expected.lat === station.lat && expected.lon === station.lon ? index : null;
}

function prediction(forecast, source, target, band, hour) {
  if (!forecast) return null;
  const tx = siteIndex(source);
  const rx = siteIndex(target);
  if (tx === null || rx === null || tx === rx) return null;
  const pair = tx * (stations.length - 1) + rx - Number(rx > tx);
  const offset = ((pair * bands.length + BAND_INDEX.get(band)) * 24 + hour) * ROW_BYTES;
  const { table } = forecast;
  const median = table.getInt32(offset, true);
  if (median === MISSING) return null;
  return {
    snr_median_db: median / 100,
    basic_muf_mhz: table.getInt16(offset + 4, true) / 100,
    snr_p10_db: (median - table.getInt16(offset + 6, true)) / 100,
    snr_p90_db: (median + table.getInt16(offset + 8, true)) / 100,
  };
}

function vector(site) {
  const lat = site.lat * STEP;
  const lon = site.lon * STEP;
  return [Math.cos(lat) * Math.cos(lon), Math.cos(lat) * Math.sin(lon), Math.sin(lat)];
}

function pathPoint(start, end, arc, fraction) {
  const before = Math.sin((1 - fraction) * arc);
  const after = Math.sin(fraction * arc);
  const [x, y, z] = start.map((value, index) => before * value + after * end[index]);
  return [Math.atan2(z, Math.hypot(x, y)) / STEP, Math.atan2(y, x) / STEP];
}

function supportOffset(frequency, muf) {
  const ratio = frequency / Math.max(0.1, muf);
  return ratio <= 1 ? 4 * (1 - ratio) : -Math.min(18, 15 * Math.log2(ratio));
}

function weatherAdjustment(source, target, arc, frequency, weather, status, now, baseline) {
  const hopCount = Math.max(1, Math.ceil(RADIUS_KM * arc / 2400));
  const halfHop = arc / (2 * hopCount);
  const horizontal = RADIUS_KM * Math.sin(halfHop);
  const curveHeight = RADIUS_KM * (1 - Math.cos(halfHop));
  const start = vector(source);
  const end = vector(target);
  const midpoints = Array.from({ length: hopCount }, (_, hop) => pathPoint(start, end, arc, (hop + 0.5) / hopCount));
  const cells = midpoints.map(([lat, lon]) => weather.cell(lat, lon, now));
  const details = {};
  let correction = 0;
  if (cells.every(Boolean)) {
    const muf = Math.min(...cells.map(({ density, height }) => {
      const secant = Math.hypot(1, horizontal / (height + curveHeight));
      return Math.sqrt(density / 1.24e10) * Math.min(3.5, secant);
    }));
    const observed = cells.filter((cell) => cell.quality > 0).length;
    let offset = supportOffset(frequency, muf);
    let confidence = 0.7 + 0.3 * observed / hopCount;
    if (baseline) {
      offset -= supportOffset(frequency, baseline.basic_muf_mhz);
      confidence = cells.reduce((sum, cell) => sum + cell.quality / 5, 0) / hopCount;
    }
    correction += offset * confidence;
    Object.assign(details, {
      muf_mhz: round(muf), glotec_at: status.glotec.observed_at,
      sampled_hops: hopCount, supported_hops: observed, glotec_confidence: round(confidence),
    });
  }
  const modeledAbsorption = status.drap.status === "live";
  if (modeledAbsorption) {
    let absorption = 0;
    for (const [hop, cell] of cells.entries()) {
      const height = cell?.height ?? 300;
      const legFraction = Math.min(1, 70 / height) / 2;
      const entry = pathPoint(start, end, arc, (hop + legFraction) / hopCount);
      const exit = pathPoint(start, end, arc, (hop + 1 - legFraction) / hopCount);
      const radius = RADIUS_KM + height;
      const elevation = Math.atan2(radius * Math.cos(halfHop) - RADIUS_KM, radius * Math.sin(halfHop));
      // A D-RAP column already includes the two traversals through the D region.
      absorption += (weather.drapLoss(...entry, frequency, now) + weather.drapLoss(...exit, frequency, now))
        / (2 * Math.sin(Math.max(10 * STEP, elevation)));
    }
    correction -= absorption;
    Object.assign(details, { drap_at: status.drap.observed_at, drap_absorption_db: round(absorption),
                             drap_quiet_reference_at: status.drap.quiet_reference_at });
  }
  if (status.kp.value !== null) {
    details.kp = status.kp.value;
    details.kp_at = status.kp.observed_at;
    if (!baseline && !modeledAbsorption) {
      const latitude = Math.max(...midpoints.map(([lat]) => Math.abs(lat)));
      correction -= 1.75 * Math.max(0, (latitude - 40) / 50) * Math.max(0, status.kp.value - 3);
    }
  }
  if (Object.keys(details).length) details.snr_adjustment_db = round(correction);
  return { correction, details };
}

class PathInputs {
  constructor(forecast, weather) {
    this.stations = stations;
    this.bands = bands;
    this.forecast = forecast;
    this.weather = weather;
  }

  status(nowMs = Date.now()) {
    const weather = this.weather.status(nowMs);
    const metadata = this.forecast?.metadata;
    const month = monthUtc(nowMs);
    const forecast = metadata && metadata.month_utc <= month
      ? { ...metadata, status: metadata.month_utc === month ? "current" : "archived" }
      : null;
    return { weather, forecast };
  }

  conditions(source, target, band, elapsedS, offsetDb = 0, nowMs = Date.now()) {
    const index = BAND_INDEX.get(band);
    if (index === undefined) throw new RangeError(`Unknown simulated band ${band}`);
    const { weather, forecast } = this.status(nowMs);
    const current = forecast?.status === "current";
    const liveWeather = weather.mode === "live" || weather.mode === "partial";
    const clock = new Date(nowMs);
    const utcHours = current || liveWeather
      ? clock.getUTCHours() + clock.getUTCMinutes() / 60 + clock.getUTCSeconds() / 3600
      : (13 + elapsedS / 3600) % 24;
    // A previous month's archive can sustain the offline scenario. Today's
    // NOAA conditions use today's heuristic when the matching month is absent.
    const selected = forecast && (current || !liveWeather)
      ? prediction(this.forecast, source, target, band, Math.floor(utcHours)) : null;
    const lat1 = source.lat * STEP;
    const lat2 = target.lat * STEP;
    const lonDelta = (target.lon - source.lon) * STEP;
    const distance = Math.sin((lat2 - lat1) / 2) ** 2
      + Math.cos(lat1) * Math.cos(lat2) * Math.sin(lonDelta / 2) ** 2;
    const arc = 2 * Math.asin(Math.sqrt(Math.min(1, distance)));
    const distanceKm = RADIUS_KM * arc;
    const daylight = [source, target].reduce((sum, station) => (
      sum + Math.max(0, Math.cos((utcHours + station.lon / 15 - 12) * Math.PI / 12))
    ), 0) / 2;
    const heuristic = [
      11 - 10 * daylight - distanceKm / 1250,
      11 - 3 * daylight - distanceKm / 2400,
      4 + 9 * daylight - distanceKm / 2600,
    ][index];
    const { correction, details } = liveWeather
      ? weatherAdjustment(source, target, arc, bands[index].frequency_mhz, this.weather, weather, nowMs, selected)
      : { correction: 0, details: {} };
    if (forecast) {
      details.forecast = {
        ...selected, status: selected ? (current ? "predicted" : "archived") : "fallback",
        engine: forecast.engine, revision: forecast.revision, month_utc: forecast.month_utc,
        noise_bandwidth_hz: 3000,
      };
    }
    const snrDb = (selected?.snr_median_db ?? heuristic) + correction + offsetDb;
    return {
      snrDb: selected ? bounded(snrDb, -100, 100) : bounded(snrDb, -25, 25),
      details: Object.keys(details).length ? details : null,
    };
  }

  refresh(options) {
    return this.weather.refresh(options);
  }
}

/** Load optional site assets, then refresh NOAA independently of the simulator. */
export async function loadPathInputs(base, { signal, fetcher, refresh = true } = {}) {
  const startup = { signal, fetcher, timeoutMs: 4000 };
  const [forecast, weather] = await Promise.all([
    loadForecast(base, startup).catch(() => null),
    readJson(new URL("weather.json", base), 512000, startup).catch(() => null),
  ]);
  const inputs = new PathInputs(forecast, new WeatherInputs(weather));
  if (refresh && !signal?.aborted) {
    const poll = () => { void inputs.refresh({ signal, fetcher }); };
    poll();
    const timer = setInterval(poll, WEATHER_POLL_MS);
    signal?.addEventListener("abort", () => clearInterval(timer), { once: true });
  }
  return inputs;
}
