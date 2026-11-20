// Offline regression checks: event parity, grid layout, circuit outputs and ray geometry.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { gunzipSync } from "node:zlib";
import { densityFraction, ray, solve } from "./ray.mjs";
const read = (name) => readFileSync(new URL(name, import.meta.url));
const json = (name) => JSON.parse(read(name));
for (const name of [
  "six",
  "global-100",
  "global-1000",
  "vara-europe",
  "vara-world",
]) {
  const r = json("data/" + name + ".json");
  if (r.directory_date) {
    assert.equal(
      new Set(r.stations.map((s) => s.name)).size,
      r.stations.length,
    );
    for (const [, a, b, ch] of r.contacts) {
      assert.notEqual(a, b);
      assert(r.stations[b].channels_hz.includes(r.frequency_hz[ch]));
    }
    if (name === "vara-world") assert.equal(r.stations.length, 191);
    if (name === "vara-europe")
      assert.deepEqual(
        r.runs[1].events.map((e) => r.stations[r.contacts[e[2]][2]].name),
        ["F5ZFX", "HB9T", "LA1B"],
      );
  }
  const blocked = new Set(r.blocked_contacts);
  for (const run of r.runs) {
    const owners = r.offers.map((o) => o.source);
    let delivered = 0,
      failures = 0;
    const occupied = new Set();
    for (const [slot, id, link, at, outcome] of run.events) {
      assert.equal(owners[id], at);
      if (outcome === "rejected") continue;
      const [when, from, to] = r.contacts[link];
      assert.equal(when, slot);
      assert.equal(at, from);
      if (run.forecast_gate) assert(!blocked.has(link));
      if (outcome !== "transfer") {
        failures++;
        continue;
      }
      for (const station of [from, to]) {
        const key = [slot, station].join("/");
        assert(!occupied.has(key), "simultaneous use of a half-duplex station");
        occupied.add(key);
      }
      owners[id] = to;
      if (to === r.offers[id].destination) delivered++;
    }
    assert.equal(delivered, run.summary.delivered);
    assert.equal(failures, run.summary.failed_calls);
    assert.equal(
      delivered +
        run.summary.expired +
        run.summary.queued +
        run.summary.rejected,
      r.offers.length,
    );
  }
  if (name === "six") {
    assert.deepEqual(
      r.runs[1].events.map((e) => [e[0] + 1, ...r.contacts[e[2]].slice(1, 3)]),
      [
        [5, 0, 3],
        [7, 3, 4],
        [10, 4, 5],
      ],
    );
    assert.equal(r.runs[0].summary.expired, 1);
  }
}
const frames = json("data/weather-index.json");
for (const f of frames) {
  const cells = JSON.parse(gunzipSync(read(f.file)));
  assert.equal(cells.length, 72 * 72);
  for (const v of cells)
    if (v)
      assert(v[0] >= 0 && v[1] >= 0 && v[2] >= 0 && v[3] >= 0 && v[3] <= 5);
}
assert.equal(JSON.parse(gunzipSync(read(frames[0].file)))[0][0], 9.0);
const forecasts = json("data/forecast.json");
assert.equal(forecasts["six/3/4/0/0"].REL, 0.86);
assert.equal(densityFraction(300, 300), 1);
assert.equal(ray(30, 300, 0, 7.1), null);
const reference = ray(30, 300, 5e11, 7.1, 768);
const solution = solve(reference.distance, 300, 5e11, 7.1);
assert(solution && Math.abs(solution.elevation - 30) < 0.02);
assert.equal(solution.path[0][1], 0);
assert.equal(solution.path.at(-1)[1], 0);
console.log(
  "Validated 16 policy runs, " +
    frames.length +
    " NOAA grids, VOACAP fixture and F2 ray checks.",
);
