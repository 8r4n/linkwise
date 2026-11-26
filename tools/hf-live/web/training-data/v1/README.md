# HF training scenario archive, version 1

Both lessons use traffic seed 42 and start at 2026-11-26 05:36:00 UTC. The
usable link runs from London to Berlin. The weak link runs from London to
Quito. Each uses the existing software Watterson channel and modem receiver.

The forecast is a six-row subset of the November 2026 cache generated with
pinned ITURHFProp v14.3 from
[upstream commit cd172be](https://github.com/ITU-R-Study-Group-3/ITU-R-HF/tree/cd172be56dc04b154e5d2fa91cbaa6ecf5284305).
It contains UTC hour 05 for 80, 40, and 20 meters on those two directed paths.
It uses 100 W isotropic transmitters, residential background noise, and a
3 kHz SNR reference. NOAA's frozen November 2026 predicted smoothed sunspot
number was 83.2, rounded to 83 for the upstream predictor.

To recreate the sparse table, retain rows with `row[3] == 5` and station keys
`("SIM-002", "SIM-008")` or `("SIM-002", "SIM-039")` in a full native
forecast. Stage the result with `tools/hf-live/site-inputs/build.py --forecast`
and compress only the generated forecast binary as gzip with mtime 0.
`forecast.json` records hashes for the engine, tables, and uncompressed
browser forecast.

`weather.json` is the staged NOAA snapshot captured around 05:34 UTC on
26 November 2026. It includes GloTEC observed at 05:05 UTC and Kp and D-RAP
observed at 05:31 UTC. The source URLs, observation and retrieval times, and
raw product hashes are retained in the snapshot. The lesson module checks the
hashes of all three packaged files.

The simulation replays archived model inputs. Receive timings and results come
from its software modem, and neither scenario represents an on-air measurement.
