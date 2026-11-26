import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const directory = pathToFileURL(resolve(process.argv[2]) + "/");
const { loadPathInputs } = await import(new URL("path-inputs.mjs", directory));
const { DRAP_URL, GLOTEC_URL, KP_URL, WeatherInputs } = await import(new URL("weather-inputs.mjs", directory));
const localFetch = async (url) => {
  try {
    return new Response(await readFile(url));
  } catch {
    return new Response("", { status: 404 });
  }
};
const inputs = await loadPathInputs(directory, { fetcher: localFetch, refresh: false });
const [source, target] = inputs.stations;
assert.equal(inputs.stations.length, 48);
assert.equal(source.id, "SIM-001");
assert.equal(inputs.stations[47].name, "Nouméa");
const date = Date.parse("2026-11-26T04:00:00Z");
const active = inputs.conditions(source, target, "40m", 0, 0, date);
assert.equal(inputs.status(date).forecast.status, "current");
assert.equal(active.snrDb, 2.25);
assert.equal(active.details.forecast.snr_p10_db, -12.5);
assert.equal(active.details.forecast.snr_p90_db, 11.75);
assert.equal(inputs.conditions(target, source, "40m", 0, 0, date).details.forecast.status, "fallback");
assert.equal(inputs.status(Date.parse("2026-10-01T00:00:00Z")).forecast, null);
const archive = inputs.conditions(source, target, "40m", 0, 0, Date.parse("2026-12-01T00:00:00Z"));
assert.equal(archive.details.forecast.status, "archived");
assert.equal(archive.details.forecast.snr_median_db, -387.24);
assert.equal(archive.snrDb, -100);

const damagedFetch = (url) => url.toString().endsWith(".bin")
  ? new Response(new Uint8Array(1624320)) : localFetch(url);
const scen = await loadPathInputs(directory, { fetcher: damagedFetch, refresh: false });
assert.equal(scen.status(date).forecast, null);
assert.equal(scen.status(date).weather.mode, "scenario");
assert.equal(scen.conditions(source, target, "40m", 0, 0, date).details, null);

const observed = new Date(Math.floor(Date.now() / 60000) * 60000 - 60000);
const iso = observed.toISOString();
const label = iso.slice(0, 16).replace("T", " ");
const grid = `glotec_icao_${iso.replace(/[-:]/g, "").slice(0, 15)}Z.geojson`;
const calls = [];
const fakeNoaa = async (url) => {
  const key = url.toString();
  calls.push(key);
  if (key === DRAP_URL) return new Response(`
# Product Valid At : ${label} UTC
# X-RAY Message : Moderate X-ray Flux
# Proton Message : Normal Proton Background
-135 -45 45 135
----------------
45 | 10 10 10 10
-45 | 10 10 10 10
`);
  if (key === KP_URL) return new Response(JSON.stringify([{ time_tag: iso, estimated_kp: 2.33 }]));
  if (key === GLOTEC_URL) return new Response(`<a href="${grid}">new grid</a>`);
  if (key === GLOTEC_URL + grid) return new Response(JSON.stringify({
    type: "FeatureCollection", time_tag: iso, features: [{
      geometry: { type: "Point", coordinates: [-177.5, -88.75] },
      properties: { NmF2: 12.4e10, hmF2: 300, quality_flag: 3 },
    }],
  }));
  return new Response("", { status: 404 });
};
const weather = new WeatherInputs();
const status = await weather.refresh({ fetcher: fakeNoaa, force: true });
assert.equal(status.mode, "live");
assert.equal(status.glotec.retrieval, "browser");
assert.equal(weather.cell(-88.75, -177.5, Date.now()).quality, 3);
assert.equal(status.kp.value, 2.33);
assert.equal(weather.drapLoss(1, 1, 10, Date.now()), 1);
assert.equal(calls.length, 4);
await weather.refresh({ fetcher: fakeNoaa });
assert.equal(calls.length, 4);
assert.equal(weather.status(Date.now() + 3600_000).mode, "fallback");
