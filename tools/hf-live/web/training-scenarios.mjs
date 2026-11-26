const defaults = { version: 1, seed: 42, startUtc: "2026-11-26T05:36:00Z", message: "Link check.", playbackRate: 8 };

export const TRAINING_SCENARIOS = Object.freeze({
  "usable-link": Object.freeze({
    ...defaults, id: "usable-link", title: "Usable link", source: "SIM-002", target: "SIM-008",
  }),
  "weak-link": Object.freeze({
    ...defaults, id: "weak-link", title: "Weak link", source: "SIM-002", target: "SIM-039",
  }),
});

export function getTrainingScenario(id) {
  return Object.hasOwn(TRAINING_SCENARIOS, id) ? TRAINING_SCENARIOS[id] : null;
}

// The compressed table contains only the six path, band, and hour rows used
// by these lessons. The standard path loader separately checks its contents.
export const TRAINING_ARCHIVE = Object.freeze({
  "forecast.json": "2200404a699a7402ec06cfe33c851a9ee26dcb783bf4d5bd969f009a669c66a9",
  "forecast-772ca55b987b8b57.bin.gz": "8aff3a57801e25c64a04534eed1ff340be59844eea735e9d2172296ac2598e8d",
  "weather.json": "7ebe87db880899cfe66e47aa1303e12170bb98435daefca87d8419fa0667e29c",
});
