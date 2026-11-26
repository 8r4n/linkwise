# Antenna comparison archive, version 1

The fixed Cairo SIM-022 to Athens SIM-038 directed route starts at
2026-11-26 05:36 UTC. The forecast contains the 05 UTC rows for that
route on 80 m, 40 m and 20 m from the native November 2026 P.533 cache.
The original 100 W isotropic medians are 7.07, 18.29 and 20.22 dB.
The original monthly cache SHA-256 is
`0519c4cf118e111fd8f5f1a70cf716db9dd957a6f84eb5c6e817694664134a39`.

The same pinned ITURHFProp model and NOAA weather byte stream as the
[training archive](../v1/README.md) are used. To reproduce the sparse forecast,
retain rows with `row[0:2] == ["SIM-022", "SIM-038"]` and `row[3] == 5` in a
native forecast cache, then stage it using `tools/hf-live/site-inputs/build.py
--forecast`. Compress the staged forecast binary as gzip with mtime 0.
