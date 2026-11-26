import assert from "node:assert/strict";
import test from "node:test";
import { chooseRadioTuning, POWER_LEVELS_W } from "./web/radio-policy.js";

const channel = (band, snrDb, options = {}) => ({ band, snrDb, busy: false, ...options });

test("tunes to an idle viable channel without wasting power", () => {
  const result = chooseRadioTuning({
    candidates: [channel("40m", 8), channel("20m", 24, { busy: true, availableInS: 2 })],
  });
  assert.equal(result.action, "transmit");
  assert.equal(result.band, "40m");
  assert.equal(result.powerW, 100);
  assert.equal(result.powerGainDb, 0);
  assert.equal(result.predictedSnrDb, 8);
});

test("uses supported power steps and the forecast's reference power", () => {
  const result = chooseRadioTuning({ candidates: [channel("20m", 2)], referenceWatts: 250 });
  assert.equal(result.powerW, 1000);
  assert.ok(Math.abs(result.powerGainDb - 10 * Math.log10(1000 / 250)) < 1e-10);
  assert.equal(result.predictedSnrDb, 8.02);
  const weak = chooseRadioTuning({ candidates: [channel("20m", -100)] });
  assert.equal(weak.action, "transmit");
  assert.equal(weak.powerW, POWER_LEVELS_W.at(-1));
  assert.equal(weak.predictedSnrDb, -90);
});

test("retunes after a near-tied band fails and raises power if no alternative is clear", () => {
  const retry = { lastBand: "40m", lastPowerW: 100, retry: 1 };
  const alternate = chooseRadioTuning({
    ...retry, candidates: [channel("40m", 8), channel("80m", 7.5)],
  });
  assert.equal(alternate.band, "80m");
  assert.match(alternate.reason, /Retuned/);
  const sameBand = chooseRadioTuning({
    ...retry, candidates: [channel("40m", 8), channel("80m", 6, { busy: true })],
  });
  assert.equal(sameBand.band, "40m");
  assert.equal(sameBand.powerW, 250);
  assert.match(sameBand.reason, /Raised power/);
});

test("waits for a briefly occupied viable band, then tries an idle band at the deadline", () => {
  const candidates = [channel("40m", -22), channel("20m", 12, { busy: true, availableInS: 1.5 })];
  const waiting = chooseRadioTuning({ candidates, elapsedS: 10, deadlineS: 12 });
  assert.equal(waiting.action, "wait");
  assert.equal(waiting.band, "20m");
  assert.equal(waiting.waitS, 1.5);
  assert.equal(waiting.powerW, null);
  const expired = chooseRadioTuning({ candidates, elapsedS: 12, deadlineS: 12 });
  assert.equal(expired.action, "transmit");
  assert.equal(expired.band, "40m");
  assert.equal(expired.powerW, 1000);
});

test("every busy channel requires bounded listen-before-talk", () => {
  const candidates = [channel("40m", 25, { busy: true, availableInS: 13 }),
                      channel("20m", 5, { busy: true, availableInS: 1 })];
  const result = chooseRadioTuning({ candidates, elapsedS: 3, deadlineS: 3 });
  assert.equal(result.action, "wait");
  assert.equal(result.band, "20m");
  assert.equal(result.waitS, 1);
  assert.equal(result.powerW, null);
  assert.equal(chooseRadioTuning({}).action, "wait");
});

test("uses recent directional SINR and actual delivery, then ages out the observations", () => {
  const candidates = [
    channel("40m", 8, { history: { attempts: 4, deliveries: 0, lastSinrDb: -5, lastAtS: 50 } }),
    channel("20m", 6, { history: { attempts: 4, deliveries: 4, lastSinrDb: 6, lastAtS: 50 } }),
  ];
  assert.equal(chooseRadioTuning({ candidates, elapsedS: 60 }).band, "20m");
  assert.equal(chooseRadioTuning({ candidates, elapsedS: 400 }).band, "40m");
});

test("does not credit the last attempt's power gain as a better RF path", () => {
  const result = chooseRadioTuning({
    candidates: [channel("20m", 4, {
      history: { attempts: 2, deliveries: 1, lastSinrDb: 10, lastPowerGainDb: 6.99, lastModeledSnrDb: 4 },
    })],
  });
  assert.equal(result.powerW, 250);
  assert.ok(result.estimatedSinrDb < result.predictedSnrDb);
});

test("is deterministic across candidate order", () => {
  const candidates = [channel("40m", 9), channel("80m", 9), channel("20m", 9)];
  const first = chooseRadioTuning({ candidates });
  const second = chooseRadioTuning({ candidates: candidates.toReversed() });
  assert.deepEqual(first, second);
});
