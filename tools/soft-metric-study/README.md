# Receiver DSP screening

Research prototypes only. No changes to the production modem or VARA wire format.
Acquisition remains at 0.20; turbo max-log decoding, interleaving and CRC are fixed.

The frozen per-run protocols, per-trial CSVs, summaries and source hashes are in
`results/`. The lab notes contain the interpretation and an A4 results chart.
Full WAV evidence, frozen prototype sources and binaries remain in the local
`2026-11-15-main-retest.sajUbP` study directory; audio is not duplicated here.

## Reproduce

Use Python with NumPy and Matplotlib, the Rust toolchain, and built release
dependencies in `rust/target/release/deps`. Supply the study directory containing
`snr-sweep/sweep` and `snr-sweep/fresh-sweep-20261117` and a new output directory:

```sh
python tools/soft-metric-study/run.py "$study" "$experiment"
python tools/soft-metric-study/challenges.py "$study" "$experiment"
python tools/soft-metric-study/bounds.py "$study" "$experiment"
```

`run.py` supports `SOFT_METHODS=baseline,track,clip` to reproduce the separate
tracking/clipping pass. `challenges.py` accepts the same variable. It defaults to
the four original soft-metric methods; specify all six for a combined run.
Existing outputs are protected against accidental replacement.

`fresh.py` consumes original WAVs and packet logs from the pinned KiwiClient
wrapper, uses macOS `afconvert`, and records payload hashes, duration and packet
continuity. The supplied fresh acquisition results describe six receptions of
four distinct payloads; they are not a known-transmission error-rate experiment.

`diversity.py` matches shared fresh transmissions and compares each single
branch with summed soft metrics. It supplies clean frame timing and frequency
estimates, isolating combining from weak-signal synchronization. `bounds.py`
separately supplies a known synthetic echo channel to an inverse equalizer and
perfect alignment to audio diversity. Those oracle assumptions are not field
implementations of pilot channel estimation or spatial synchronization.

`adjudicate.py` reproduces the unexpected CRC-valid non-beacon outputs in the
tracking/clipping challenge set. `report.py SOFT_EXPERIMENT TRACKING_EXPERIMENT`
archives compact results and draws the A4 chart. These scripts read local WAVs;
they do not transmit radio signals or invoke stock VARA.

## Findings and boundaries

On the later 80-trial AWGN corpus, baseline/tone/track/clip recover 60, symbol
weighting 59, and combined weighting 58 exact payloads. Per-tone weighting helps
the synthetic interferer and echo cases but loses frames elsewhere. It needs a
separately validated gating or noise model before production consideration.

Real two-Kiwi combining at a recording/added-noise ratio of 0 dB recovers 20/20
versus 17/20 on the better single branch, with clean synchronization supplied.
Both remain 0/20 at -8 and -4 dB in this small selected corpus.

No production candidate is promoted. Still unverified: independent broad-corpus
gains, calibrated RF sensitivity, a sample-clock tracking loop, pilot-estimated
MMSE/sequence equalization, and fully independent weak-signal diversity
acquisition. ITU-R F.1487/Watterson qualification and mixed stock interoperability
must precede promotion of any future candidate. The deterministic challenge
models here are explicitly not an ITU channel implementation.

Stock VARA4.9.1 separately passed all six opening/middle/end clean controls
with a fixed15s startup wait and recovered exactly the same three payloads
per fresh capture as Linkwise. See `results/stock-qualification.json`. This
qualifies only this replay run; it does not explain prior startup misses or
retroactively qualify an earlier SNR sweep. `stock_score.py` validates guest
hashes, full playback positions, version and KISS payload sets.

## Fresh best-site retest, 18 November 2026 UTC

`results/best-sites-2026-11-18/` contains frozen protocols and hash-only results.
The new gate requires a peak/median tone floor ratio of at least4 in both
frame halves, consistent peak bins, and baseline FEC failure before applying
one-sided weights. It preserves167/240 ordinary AWGN successes and gains49/60
under the synthetic narrowband interferer. One reproduced unmatched CRC-valid
non-beacon body blocks production adoption. No modem code was changed.

Four new180s captures at Westminster and Warrenton produced18 baseline
receptions representing16 payloads, without observed sequence gaps/overload.
The initial shortlist attempts at Chichester, Glava, Hameenlinna, Free Union,
Newton and Westminster's Beverage feed produced no usable audio. Those
connection failures are retained separately from RF/antenna performance.

The primary diversity receiver searches noisy input pilots for timing and
CFO; it receives no clean calibration. Matched event windows are supplied,
so it is not autonomous continuous cross-site association. Equal-weight
combining loses16/20 trials against the stronger branch at-8dB. A separately
labelled adaptive lower-SNR diagnostic compares this with supplied clean
calibration. `timing_candidate` fields indicate proximity to a baseline
accepted window, not an independent ground-truth symbol epoch.

The research workflow is `collect-best.py`, `build-best.py`, `smoke-best.py`,
`prepare-best.py`, `evaluate-best.py`, `diversity-best.py`, and
`extend-diversity-best.py`. The collector needs the pinned packet-observation
`record.py` wrapper in its output root. `best_common.py` defines deterministic
quantized trial generation. `make-stock-best.py` builds the bounded stock
subset; the local Windows runner uses the existing isolated audio bench.
`score-stock-best.py` checks input hashes, stereo identity, completed playback,
version and KISS equality. `adjudicate-best.py` and `natural-best.py` prepare
separate diagnostic replays after the primary stock run has completed.
`continuous-best.py` checks entire original captures, and `report-best.py`
archives results and draws the A4 chart. Each original WAV is converted and
replayed separately, including after a capture spans multiple files. Captures
with several files are excluded from diversity pairing because the packet log
cannot establish a start time for each file. Full audio remains local.
