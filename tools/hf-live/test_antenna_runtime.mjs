import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { AntennaRuntime } from "./web/antenna-runtime.mjs";

const catalog = JSON.parse(await readFile(new URL("./antenna-profiles.json", import.meta.url), "utf8"));
const stations = [
  { id: "SIM-022", lat: 30.04, lon: 31.24 },
  { id: "SIM-038", lat: 37.98, lon: 23.73 },
];

test("switching and physical heading settle independently along the shortest arc", () => {
  const model = new AntennaRuntime(catalog, stations, [{
    station: "SIM-022", txPort: "yagi-20m", azimuth: 350, fixedAzimuth: 217,
  }]);
  assert.equal(model.configure({ station: "SIM-022", rxPort: "active-loop", azimuth: 10 }, 0).state, "switching");
  model.advance(1, () => {});
  assert.equal(model.summary(0).actualAzimuth, 356);
  assert.equal(model.summary(0).targetAzimuth, 10);
  assert.equal(model.summary(0).fixedAzimuth, 217);
  assert.equal(model.endpoint(0, "rx").heading_deg, 217);
  model.advance(4, () => {});
  assert.equal(model.summary(0).state, "ready");
  assert.equal(model.summary(0).actualAzimuth, 10);
  assert.equal(model.endpoint(0, "rx").heading_deg, 217);
  assert.equal(model.endpoint(0, "tx").heading_deg, 10);
});

test("TX roles, malformed commands, and incompatible manual bands stay explicit", () => {
  const model = new AntennaRuntime(catalog, stations);
  assert(model.configure({ station: "SIM-022", txPort: "active-loop" }, 0).error);
  assert(model.configure({ station: "missing", txPort: "isotropic" }, 0).error);
  assert(model.configure({ station: "SIM-022", azimuth: 361 }, 0).error);
  model.configure({ station: "SIM-022", txPort: "yagi-20m", azimuth: 12 }, 0, true);
  assert.equal(model.plan(0, 1, "40m"), null);
  assert.equal(model.plan(0, 1, "20m").tx.profile_id, "yagi-20m");
});

test("automatic retries favor proper bearings and preserve first-attempt heading", () => {
  const model = new AntennaRuntime(catalog, stations, [
    { station: "SIM-022", txPort: "yagi-20m", azimuth: 144, autoAim: true },
    { station: "SIM-038", rxPort: "yagi-20m", azimuth: 320 },
  ]);
  const first = model.plan(0, 1, "20m", 0);
  assert.equal(first.tx.heading_deg, 144);
  const retry = model.plan(0, 1, "20m", 1);
  assert(retry.gainDb > first.gainDb);
  assert.equal(retry.rx.heading_deg, first.rx.heading_deg);
  assert.equal(model.prepare(0, 1, retry, 0, () => {}), false);
  model.advance(35, () => {});
  assert.equal(model.prepare(0, 1, retry, 35, () => {}), true);
  assert.equal(model.plan(0, 1, "40m", 1), null, "a fixed receive port can still rule out the band");
  model.configure({ station: "SIM-038", autoAim: true }, 35);
  assert(model.plan(0, 1, "40m", 1), "autoAim may choose another compatible port");
});

test("receiver heading changes wanted gain without changing directional transmit EIRP", () => {
  const model = new AntennaRuntime(catalog, stations);
  const neutral = model.budget(0, 1, "20m", 100);
  assert.equal(neutral.netGainDb, 0);
  assert.equal(neutral.eirpW, 100);
  const tx = { profile_id: "yagi-20m", heading_deg: neutral.tx.bearingDeg };
  const rx = { profile_id: "yagi-20m", heading_deg: neutral.rx.bearingDeg };
  const aligned = model.budget(0, 1, "20m", 100, tx, rx);
  const turned = model.budget(0, 1, "20m", 100, tx, { ...rx, heading_deg: (rx.heading_deg + 180) % 360 });
  assert.equal(aligned.eirpW, turned.eirpW);
  assert(Math.abs(aligned.netGainDb - turned.netGainDb - 20) < 1e-9);
});
