// Decode the staged NOAA snapshot and refresh it directly where CORS allows.
export const GLOTEC_URL = "https://services.swpc.noaa.gov/products/glotec/geojson_2d_urt/";
export const KP_URL = "https://services.swpc.noaa.gov/json/planetary_k_index_1m.json";
export const DRAP_URL = "https://services.swpc.noaa.gov/text/drap_global_frequencies.txt";
const WIDTH = 72;
const HEIGHT = 72;
const CELL_SIZE = 9;
const DAY_MS = 86400000;
const decoder = new TextDecoder("utf-8", { fatal: true });

const sources = {
  glotec: { url: GLOTEC_URL, age: 3600, poll: 600000 },
  kp: { url: KP_URL, age: 900, poll: 120000 },
  drap: { url: DRAP_URL, age: 900, poll: 120000 },
};

export async function readBytes(url, maxBytes, { signal, fetcher = globalThis.fetch, timeoutMs = 6000 } = {}) {
  if (signal?.aborted) throw new Error("Data request cancelled");
  const controller = new AbortController();
  const abort = () => controller.abort();
  signal?.addEventListener("abort", abort, { once: true });
  const timer = setTimeout(abort, timeoutMs);
  try {
    const response = await fetcher(url, {
      cache: "no-cache", credentials: "omit", signal: controller.signal,
    });
    if (!response.ok || Number(response.headers.get("content-length")) > maxBytes || !response.body) {
      abort();
      throw new Error(`HF input request failed (${response.status})`);
    }
    const reader = response.body.getReader();
    const chunks = [];
    let length = 0;
    try {
      while (true) {
        const { value, done } = await reader.read();
        if (done) break;
        length += value.byteLength;
        if (length > maxBytes) {
          abort();
          throw new Error("HF input exceeds its size limit");
        }
        chunks.push(value);
      }
    } finally {
      reader.releaseLock();
    }
    const bytes = new Uint8Array(length);
    let offset = 0;
    for (const chunk of chunks) {
      bytes.set(chunk, offset);
      offset += chunk.byteLength;
    }
    return bytes;
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener("abort", abort);
  }
}

export async function readJson(url, maxBytes, options) {
  return JSON.parse(decoder.decode(await readBytes(url, maxBytes, options)));
}

export async function sha256(bytes) {
  const hash = await globalThis.crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(hash), (value) => value.toString(16).padStart(2, "0")).join("");
}

const utc = (value) => new Date(value).toISOString();
const number = (value, low, high) => Number.isFinite(value) && value >= low && value <= high;
const wrap = (value) => ((value % 360) + 360) % 360;
const gridColumn = (lon) => Math.floor(wrap(lon + 180) / 5);
const gridRow = (lat) => Math.min(HEIGHT - 1, Math.floor((lat + 90) / 2.5));

function assetBytes(base64, length) {
  if (typeof base64 !== "string" || base64.length !== Math.ceil(length / 3) * 4) {
    throw new Error("Invalid HF grid length");
  }
  const bytes = Uint8Array.from(atob(base64), (character) => character.charCodeAt(0));
  if (bytes.length !== length) throw new Error("Invalid HF grid length");
  return new DataView(bytes.buffer);
}

function staticProduct(product, source) {
  const observed = Date.parse(product.observed_at);
  const fetched = Date.parse(product.fetched_at);
  if (!Number.isFinite(observed) || !Number.isFinite(fetched) || observed > fetched
      || !/^[0-9a-f]{64}$/.test(product.sha256 ?? "") || typeof product.source !== "string"
      || !product.source.startsWith(source)) {
    throw new Error("Invalid weather product provenance");
  }
  return { observed, fetched, source: product.source, digest: product.sha256, retrieval: "site" };
}

function staticGrid(kind, product) {
  if (!product) return null;
  const publicSource = kind === "quiet_drap" ? DRAP_URL : sources[kind].url;
  const base = staticProduct(product, publicSource);
  if (kind === "glotec") return {
    ...base, cells: assetBytes(product.cells, WIDTH * HEIGHT * CELL_SIZE),
    usable: product.usable_cells, observedCells: product.observed_cells,
  };
  if (kind === "kp") {
    if (!number(product.value, 0, 9)) throw new Error("Invalid Kp estimate");
    return { ...base, value: product.value };
  }
  const { rows, cols, quiet } = product;
  if (!Number.isInteger(rows) || !number(rows, 2, 180) || !Number.isInteger(cols)
      || !number(cols, 2, 360) || typeof quiet !== "boolean") throw new Error("Invalid D-RAP grid");
  return { ...base, rows, cols, quiet, cells: assetBytes(product.cells, rows * cols * 2) };
}

function validCoordinates(lat, lon) {
  return number(lat, -90, 90) && number(lon, -180, 180);
}

function decodeGlotec(document, expectedTime) {
  if (document?.type !== "FeatureCollection" || Date.parse(document.time_tag) !== expectedTime
      || !Array.isArray(document.features)) throw new Error("Invalid GloTEC grid metadata");
  const cells = new DataView(new ArrayBuffer(WIDTH * HEIGHT * CELL_SIZE));
  for (let offset = 8; offset < cells.byteLength; offset += CELL_SIZE) cells.setUint8(offset, 255);
  let usable = 0;
  let observedCells = 0;
  for (const feature of document.features) {
    const coords = feature?.geometry?.coordinates;
    const info = feature?.properties;
    if (feature?.geometry?.type !== "Point" || !Array.isArray(coords) || coords.length !== 2 || !info) continue;
    const [lon, lat] = coords;
    if (!validCoordinates(lat, lon)) continue;
    const col = gridColumn(lon);
    const row = gridRow(lat);
    const { NmF2: density, hmF2: height, quality_flag: quality } = info;
    if (lon !== -177.5 + 5 * col || lat !== -88.75 + 2.5 * row
        || !number(density, 1, 1e15) || !number(height, 1, 1000)
        || !Number.isInteger(quality) || !number(quality, 0, 5)) continue;
    const offset = (row * WIDTH + col) * CELL_SIZE;
    if (cells.getUint8(offset + 8) !== 255) throw new Error("Duplicate GloTEC cell");
    cells.setFloat32(offset, density, true);
    cells.setFloat32(offset + 4, height, true);
    cells.setUint8(offset + 8, quality);
    usable += 1;
    observedCells += Number(quality > 0);
  }
  if (!usable) throw new Error("GloTEC supplied no usable cells");
  return { cells, usable, observedCells };
}

async function provenance(observed, source, data, fetched = Date.now()) {
  if (!Number.isFinite(observed) || observed > fetched) throw new Error("Invalid NOAA product time");
  return { observed, fetched, source, digest: await sha256(data), retrieval: "browser" };
}

function glotecTime(name) {
  const match = name.match(/_(\d{4})(\d{2})(\d{2})T(\d{2})(\d{2})(\d{2})Z/);
  return Date.parse(`${match[1]}-${match[2]}-${match[3]}T${match[4]}:${match[5]}:${match[6]}Z`);
}

async function fetchGlotec(previous, options) {
  const listing = decoder.decode(await readBytes(GLOTEC_URL, 2_000_000, options));
  const names = Array.from(new Set(listing.match(/glotec_icao_\d{8}T\d{6}Z\.geojson/g) ?? [])).sort().reverse();
  const now = Date.now();
  let attempts = 0;
  for (const name of names) {
    const observed = glotecTime(name);
    if (!Number.isFinite(observed) || observed > now) continue;
    if (previous && previous.observed >= observed) return previous;
    if (++attempts > 3) break;
    const source = new URL(name, GLOTEC_URL).href;
    try {
      const data = await readBytes(source, 4_000_000, options);
      const grid = decodeGlotec(JSON.parse(decoder.decode(data)), observed);
      return { ...await provenance(observed, source, data), ...grid };
    } catch {
      if (options.signal?.aborted) break;
    }
  }
  throw new Error("No valid GloTEC grid");
}

function parseKpTime(text) {
  if (typeof text !== "string") return NaN;
  return Date.parse(/(?:Z|[+-]\d\d:\d\d)$/i.test(text) ? text : `${text}Z`);
}

async function fetchKp(options) {
  const data = await readBytes(KP_URL, 128_000, options);
  const rows = JSON.parse(decoder.decode(data));
  const fetched = Date.now();
  if (!Array.isArray(rows)) throw new Error("Invalid Kp estimate");
  let latest = null;
  for (const row of rows) {
    const observed = parseKpTime(row?.time_tag);
    const value = row?.estimated_kp;
    if (number(value, 0, 9) && observed <= fetched && (!latest || observed > latest.observed)) {
      latest = { observed, value };
    }
  }
  if (!latest) throw new Error("No valid Kp estimate");
  return { ...await provenance(latest.observed, KP_URL, data, fetched), value: latest.value };
}

function wholeAxis(values, extent) {
  const step = extent / values.length;
  return values.length >= 2 && values.every((value, index) => Math.abs(value - (-extent / 2 + step * (index + 0.5))) < 1e-6);
}

export function parseDrap(text) {
  const stamp = text.match(/^# Product Valid At\s*:\s*(\d{4}-\d{2}-\d{2})\s+(\d{2}:\d{2}) UTC\s*$/m);
  if (!stamp) throw new Error("Missing D-RAP validity time");
  const content = text.split(/\r?\n/).map((line) => line.trim())
    .filter((line) => line && !line.startsWith("#") && !/^-+$/.test(line));
  if (!content.length) throw new Error("Missing D-RAP frequencies");
  const longitudes = content[0].split(/\s+/).map(Number);
  const records = content.slice(1).map((line) => {
    const row = line.match(/^([+-]?\d+(?:\.\d+)?)\s*\|\s*(.+)$/);
    if (!row) throw new Error("Invalid D-RAP row");
    const values = row[2].split(/\s+/).map(Number);
    if (values.length !== longitudes.length || !values.every((value) => number(value, 0, 100))) {
      throw new Error("Invalid D-RAP value");
    }
    return { lat: Number(row[1]), values };
  }).sort((a, b) => a.lat - b.lat);
  if (!wholeAxis(longitudes, 360) || !wholeAxis(records.map((row) => row.lat), 180)) {
    throw new Error("D-RAP does not cover a regular global grid");
  }
  const cells = new DataView(new ArrayBuffer(records.length * longitudes.length * 2));
  for (const [index, value] of records.flatMap((row) => row.values).entries()) cells.setUint16(index * 2, Math.round(value * 10), true);
  const quiet = /^#\s+X-RAY Message\s*:\s*Normal X-ray Background\s*$/im.test(text)
    && /^#\s+Proton Message\s*:\s*Normal Proton Background\s*$/im.test(text);
  return { observed: Date.parse(`${stamp[1]}T${stamp[2]}:00Z`), cells, quiet, rows: records.length, cols: longitudes.length };
}

async function fetchDrap(options) {
  const data = await readBytes(DRAP_URL, 512_000, options);
  const parsed = parseDrap(decoder.decode(data));
  return { ...parsed, ...await provenance(parsed.observed, DRAP_URL, data) };
}

function columnLoss(grid, lat, lon, frequency) {
  const row = Math.min(grid.rows - 1, Math.floor((lat + 90) / 180 * grid.rows));
  const col = Math.floor(wrap(lon + 180) / 360 * grid.cols);
  const haf = grid.cells.getUint16((row * grid.cols + col) * 2, true) / 10;
  return (haf / frequency) ** 1.5;
}

export class WeatherInputs {
  #items = { glotec: null, kp: null, drap: null };
  #quiet = null;
  #lastPoll = {};
  #errors = {};
  #active = null;
  #didPoll = false;

  constructor(document) {
    if (document?.schema !== 1) return;
    for (const key of [...Object.keys(sources), "quiet_drap"]) {
      try {
        const product = staticGrid(key, document[key]);
        if (key === "quiet_drap") this.#quiet = product?.quiet ? product : null;
        else this.#items[key] = product;
      } catch {
        // Other products remain usable when one staged source is malformed.
      }
    }
  }

  #fresh(key, now) {
    const item = this.#items[key];
    return item && item.fetched <= now && item.observed <= now && now - item.observed <= sources[key].age * 1000
      ? item : null;
  }

  #quietReference(now) {
    const drap = this.#fresh("drap", now);
    if (!this.#quiet || !drap || this.#quiet.fetched > now) return null;
    const delta = drap.observed - this.#quiet.observed;
    return delta >= 0 && delta <= DAY_MS ? this.#quiet : null;
  }

  status(now = Date.now()) {
    const state = {};
    let fresh = 0;
    for (const [key, source] of Object.entries(sources)) {
      const item = this.#items[key];
      const available = this.#fresh(key, now);
      fresh += Number(Boolean(available));
      state[key] = {
        status: available ? "live" : item ? "stale" : "unavailable",
        observed_at: item ? utc(item.observed) : null,
        fetched_at: item ? utc(item.fetched) : null,
        age_s: item ? Math.round((now - item.observed) / 1000) : null,
        max_age_s: source.age,
        source: item?.source ?? source.url,
        sha256: item?.digest ?? null,
        retrieval: item?.retrieval ?? null,
        error: this.#errors[key] ?? null,
      };
    }
    state.glotec.usable_cells = this.#items.glotec?.usable ?? null;
    state.glotec.observed_cells = this.#items.glotec?.observedCells ?? null;
    state.glotec.total_cells = WIDTH * HEIGHT;
    state.kp.kind = "observation-derived estimate";
    state.kp.value = this.#fresh("kp", now)?.value ?? null;
    state.drap.kind = "modeled solar X-ray and proton absorption";
    state.drap.quiet = this.#items.drap?.quiet ?? null;
    const quiet = this.#quietReference(now);
    state.drap.quiet_reference_at = quiet ? utc(quiet.observed) : null;
    const hasArchive = Object.values(this.#items).some(Boolean);
    const mode = fresh === 3 ? "live" : fresh ? "partial" : hasArchive || this.#didPoll ? "fallback" : "scenario";
    return { ...state, mode, origin: fresh ? "live" : hasArchive ? "archive" : "scenario" };
  }

  cell(lat, lon, now) {
    const product = this.#fresh("glotec", now);
    if (!product || !validCoordinates(lat, lon)) return null;
    const offset = (gridRow(lat) * WIDTH + gridColumn(lon)) * CELL_SIZE;
    const quality = product.cells.getUint8(offset + 8);
    if (!number(quality, 0, 5)) return null;
    const density = product.cells.getFloat32(offset, true);
    const height = product.cells.getFloat32(offset + 4, true);
    return number(density, 1, 1.01e15) && number(height, 1, 1000) ? { density, height, quality } : null;
  }

  drapLoss(lat, lon, frequency, now) {
    const product = this.#fresh("drap", now);
    if (!product || !validCoordinates(lat, lon) || !number(frequency, 3, 30)) return null;
    if (product.quiet) return 0;
    const baseline = this.#quietReference(now);
    const quietLongitude = baseline ? lon + (product.observed - baseline.observed) / 240000 : lon;
    const quietLoss = baseline ? columnLoss(baseline, lat, quietLongitude, frequency) : 0;
    return Math.max(0, columnLoss(product, lat, lon, frequency) - quietLoss);
  }

  refresh({ signal, fetcher, force = false } = {}) {
    if (signal?.aborted) return Promise.resolve(this.status());
    if (this.#active) return this.#active;
    const now = Date.now();
    const options = { signal, fetcher };
    const getters = { glotec: () => fetchGlotec(this.#items.glotec, options),
                      kp: () => fetchKp(options), drap: () => fetchDrap(options) };
    const tasks = [];
    for (const [key, source] of Object.entries(sources)) {
      const last = this.#lastPoll[key] ?? 0;
      const item = this.#items[key];
      const interval = this.#errors[key] ? Math.min(source.poll, 120000) : source.poll;
      if (!force && now - Math.max(last, item?.fetched ?? 0) < interval) continue;
      this.#lastPoll[key] = now;
      tasks.push(getters[key]().then((result) => {
        if (!this.#items[key] || this.#items[key].observed <= result.observed) {
          this.#items[key] = result;
          if (key === "drap" && result.quiet) this.#quiet = result;
        }
        delete this.#errors[key];
      }).catch(() => {
        if (!signal?.aborted) this.#errors[key] = "NOAA refresh unavailable";
      }));
    }
    this.#active = Promise.all(tasks).then(() => {
      this.#active = null;
      if (tasks.length) this.#didPoll = true;
      return this.status();
    });
    return this.#active;
  }
}
