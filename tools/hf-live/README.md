# Live HF station simulator

The GitHub Pages [Learn page](https://8r4n.github.io/linkwise/learn/) links to
four browser-local missions. Learners compare TX and RX, deliver an
authenticated message, diagnose a weak link, and compare BW500 with BW2300.
Training runs use bundled channel inputs and a fixed simulation clock.
The Missions panel saves progress in the browser and offers a reset for
another practice run.

## Antennas and link budget

Open [Antenna Lab](https://8r4n.github.io/linkwise/hf-simulation/?lab=antenna)
after completing **Read a burst**. It uses matched BW500 transmissions on an
archived 20 m link. Compare dipole and Yagi transmission and then change only
the receiving antenna. Each trial retains its original antenna configuration
with the SigPlot capture. The lab saves its two completions in the browser.

The [antenna-enabled free simulation](https://8r4n.github.io/linkwise/hf-simulation/?antennas=1)
offers separate transmit and receive ports for each station. Choose a peer to
see its bearing or aim a rotatable antenna. Commands settle between bursts.
Automatic aiming can choose a compatible port and heading on a retry.
The globe can display TX, RX, or both gain contours across the network at a
shared elevation. A selected link has a closer view using its modeled
elevation. Contour size is a relative gain display.

The catalog in `antenna-profiles.json` records representative antenna
patterns and links to their sources. [Antenna modeling notes](antenna-profiles.md)
describe how to export a pattern with 4nec2. The [KiwiSDR study](../../reference/kiwisdr-antenna-study/2026-11-18-summary.json)
informs the available installation types. Its waterfall statistics describe
observed receive systems. They are independent from the catalog gain values.
The model evaluates each endpoint pattern using the short-path bearing and a
representative skywave elevation. Directional EIRP is conducted watts times
the transmit antenna's linear gain toward the peer times feedline efficiency.
The receiving antenna and feedline change the wanted signal and the separately
modeled co-channel signals against the receiver's reference noise.

Native runs can set `--antenna-preset kiwi-inspired` or use
`--antenna-config PATH` to set station ports, headings, and conducted power.
For example, a configuration can set the transmit heading and receive feed at
one station while other stations use the isotropic reference.

```json
{"schema":1,"stations":{"SIM-001":{"tx":{"profile_id":"yagi-20m","heading_deg":80},"rx":{"profile_id":"tci-530","heading_deg":0},"power_w":100}}}
```

## Station traffic

Run autonomous stations on three shared simulated HF channels and watch their
packet attempts on a live Cesium globe. The default run starts 24 synthetic
stations. A station generates its own traffic, chooses an available band using
its observed outcomes, and backs off and retries up to twice. Each station can
participate in one burst at a time. The shared-medium option lets separate
station pairs transmit on the same band.

## Run locally

Use Python 3.10 or later with the GNU Radio 3.10 Python bindings, NumPy, and SciPy
for the Watterson profile. An isolated conda environment provides these
dependencies on macOS or Linux.

```sh
conda create -n linkwise-hf -c conda-forge gnuradio-core=3.10.12 numpy scipy
conda activate linkwise-hf
cargo build --release --locked --manifest-path rust/Cargo.toml --bin linkwise
python globe/prepare.py
python tools/hf-live/serve.py --stations 48 --seed 42 --modem rust/target/release/linkwise \
  --channel-model watterson-moderate --medium shared
```

Open [the live console](http://127.0.0.1:8752/) in a WebGL-capable browser.
The server binds to loopback by default. The Cesium installer verifies the
pinned distribution. The globe serves NASA's cloud-free Blue Marble Next
Generation satellite composite from August 2004. Natural Earth is available
if the satellite image fails to load. Map assets are served locally.
[Image sources and processing](../../globe/imagery/README.md)

Choose 2–240 stations with `--stations`. The first 48 occupy illustrative sites
worldwide. Additional stations are clustered near those sites. Use `--band 40m`
to put all stations on one shared frequency. `--speed 3`
advances three simulated seconds per wall-clock second when the host can keep up.
For a finite run, set `--duration 180` to stop starting bursts after 180 simulated
seconds and finish any bursts still in flight. The web server stays available for
inspection until stopped with Ctrl+C. For a batch run, add `--headless` to get a
JSON result at full speed without starting HTTP or needing Cesium.

```sh
python tools/hf-live/serve.py --headless --stations 48 --seed 42 --duration 180
```

In native BW500 mode every station uses the Linkwise CLI to encode and decode
its bursts. GNU Radio combines propagation, noise, and interference before
decoding each finite frame. Use the Rocky Linux Linkwise RPM's
`/usr/bin/linkwise` for production-baseline runs and the release build for
local development.

## Read the live stream

An active link reserves its transmitting and receiving stations for the
encoded burst's airtime. The dashboard shows exact-byte deliveries, failed
decodes, and active attempts. Selecting a row shows its path.
Select a station marker or use the Station control to open its latest
transmission in SigPlot. **TX** shows the original encoder output. **RX**
pins that same attempt and shows the samples at its intended receiver,
including fading, noise, and overlapping signals. **Follow latest** resumes
tracking the station's transmissions. The panel also shows the receiver's
measured SNR, SINR, and decode result. **Signal** stacks the spectrum above
the waterfall with linked frequency zoom. **Waveform** shows amplitude versus
time. The spectrum and waterfall
show modeled RF frequency in MHz for the burst's band. BPSK uses a unity cosine
mixer with two sidebands around the simulated channel center. BW500 uses upper
sideband with its dial 1,500 Hz below the center. The dBFS reference is a
full-scale, bin-centered sine at the input to the modeled mixer.
[SigPlot streaming example](https://sigplot.lgsinnovations.com/examples/realtime.html)

On phones and narrow tablets, **View signal** jumps from the station map to
the plots. The frequency buttons pan and zoom both charts together. The default
view shows the RF channel center and the nearby sidebands. Zooming out reveals
the full modeled RF span. Use **Level range** to widen the dBFS display, and tap
or click the spectrum to read the nearest RF frequency bin.

### Upstream SigPlot review

- The homepage gallery's 19 modes cover fixed waveforms, constellations,
  spectra, and realtime traces and rasters. The eight developer pages cover
  overlays, streaming, audio, menus, MTAG, selection, appearance, and plugins.
  Linkwise uses the streaming example's paired plots, persistent layer handles,
  and incremental updates; MTAG supplies the frequency-bin readout.
  [Gallery](https://sigplot.lgsinnovations.com/) · [Streaming](https://sigplot.lgsinnovations.com/examples/realtime.html) · [MTAG](https://sigplot.lgsinnovations.com/examples/mtag.html)
- The 2.0 preview's In Action panel and editable example sources illustrate
  raster palettes, color scales, and selection. Some legacy editors embed HTTP
  resources that modern browsers block. Linkwise exposes pan, zoom, and dBFS
  display range through visible controls on both phone and desktop.
  [In Action](https://sigplot.lgsinnovations.com/sigplot-2.0/inaction.html) · [Editable example list](https://sigplot.lgsinnovations.com/sigplot-2.0/tryout.html)
- The linked GitHub branch contains an older release candidate and a basic
  array-overlay example. Linkwise uses its bundled SigPlot 3.1.7, with FFT bin
  centers shared by the trace and raster, and explicitly advances the waterfall
  time view when a user has zoomed the linked frequency axis.
  [Source repository](https://github.com/LGSInnovations/sigplot/tree/480cec393280a41359924383d06bb6849eeab879) · [Pinned 3.1.7 package](https://registry.npmjs.org/sigplot/-/sigplot-3.1.7.tgz)

The `/events` endpoint uses Server-Sent Events. `snapshot` contains stations,
bands, live links, recent results, and counters. `attempt` carries each start
and result, and `status` reports simulated time and totals. A reconnect receives
a fresh snapshot. `retried` counts repeat transmissions, while `dropped` counts
messages that exhausted their retries or could not enter a full queue.

`GET /waveforms/<station-id>` returns the latest TX waveform. Use
`?stage=rx&attempt=<id>` to fetch its matched receiver input after completion.
Samples are little-endian float32 encoded as base64. The `analysis` field
contains a one-sided Hann-windowed spectrum averaged in linear power and
a time-major waterfall. The server keeps the latest pair per transmitter and
the 16 most recently superseded pairs. HTTP 204 indicates pending RX or an
unchanged capture requested with `after=<id>`. HTTP 404 indicates an absent
or expired capture.

## Signal and channel model

- Each burst carries nine generated bytes, including a message ID, source and
  destination IDs, a format version, and a synthetic measurement. By default,
  the transmitter sends an aligned 250 bit/s BPSK frame with a CRC-32. GNU Radio
  applies time-varying attenuation and seeded Gaussian noise, then integrates
  the received symbols. A delivery requires a valid frame and exact bytes.
- Native BW500 sends one 5.376-second level-1 burst at 48 kHz. Its default,
  `--channel-model watterson-moderate`, uses the ITU-R F.1487 moderate
  mid-latitude profile, with equal-mean-power independent Gaussian paths at 0
  and 1 ms and 0.5 Hz Doppler spread (two standard deviations). Fading
  stays coherent across bursts on the same directed link and band. Noise
  is calibrated with a 300–3300 Hz measurement filter against the clean
  ensemble signal reference. The instantaneous SNR can vary with the fade.
  `--channel-model flat` retains the legacy whole-band SNR convention.
  [Calibration protocol](../../rust/docs/kiwisdr-test-plan.md#19-reference-hf-simulator-calibration)
- `--medium shared` permits up to two transmissions on a band. The receiver
  adds time-aligned, independently propagated same-band signals from other
  transmitters, scaled by their predicted level at that receiver.
  `--interference tone`, `impulse`, or `mixed` adds seeded bench disturbances.
  `--medium exclusive` and `--interference none` are the default controls.
  The scheduler models burst boundaries and keeps endpoints half-duplex.
- Interactive runs use current UTC and poll NOAA every two minutes. GloTEC
  supplies a quality-weighted estimate of F2 support, D-RAP adds event
  absorption, and planetary Kp provides disturbance context. Stale products
  lose their effect. Headless runs use a deterministic scenario by default.
  Use `--weather live` for a current fetch or `--weather-replay <manifest>`
  for a recorded quiet or disturbed period.
  [GloTEC](https://www.spaceweather.gov/products/glotec) · [D-RAP](https://www.spaceweather.gov/products/d-region-absorption-predictions-d-rap)

## Add a P.533 path baseline

For native BW500 with the Watterson profile, pass `--forecast <cache.json>`
to use monthly P.533 predictions for matching station coordinates and UTC hours.
Build the [pinned ITURHFProp engine](https://github.com/ITU-R-Study-Group-3/ITU-R-HF/tree/cd172be56dc04b154e5d2fa91cbaa6ecf5284305)
with its monthly data files, then generate the cache outside the live process.
The generator records the executable, library, and table digests, transmitter
power, antenna assumptions, and the bundled P.372 noise inputs. New calendar
months and missing path-hour rows use the existing path heuristic until a
matching cache is available. The same NOAA corrections operate on both paths.

```sh
python tools/hf-live/path_forecast.py --itu-root /path/to/ITU-R-HF \
  --start-utc 2026-11-26T00:00:00Z --stations 48 --jobs 4 \
  --noaa-solar-cycle /path/to/predicted-solar-cycle.json --output forecast.json
```

Use NOAA's [monthly solar-cycle product](https://services.swpc.noaa.gov/json/solar-cycle/predicted-solar-cycle.json)
as the frozen input, or provide an explicit scenario value with `--sunspots`.
Weather replay uses a JSON manifest with `schema: 1`, `start_utc`, and
`records`. Each record gives a `product` (`glotec`, `kp`, or `drap`), a
`file` relative to the manifest, its UTC `fetched_at` timestamp, and its
`sha256`. Replay uses the original freshness windows and applies the
record at its retrieval time. [NOAA D-RAP archive](https://www.ncei.noaa.gov/products/space-weather/ionospheric-program/d-region-absorption-prediction)

## Verify the channel

```sh
LINKWISE_HF_TEST_MODEM=/usr/bin/linkwise python -m unittest discover -s tools/hf-live
python tools/hf-live/calibrate.py /tmp/linkwise-f1487-calibration
python tools/hf-live/benchmark.py --modem /usr/bin/linkwise --seed 42 --trials 24
python tools/hf-live/serve.py --headless --stations 48 --seed 42 --duration 180 \
  --modem /usr/bin/linkwise --channel-model watterson-moderate --medium shared
```

The benchmark sends the same nine-byte frames and calibrated noise to both
channel profiles. It reports paired exact-byte outcomes and measured SNR.
The 48-station headless run reports simulated and wall-clock time,
overlapped attempts, errors, and totals. A real-time server additionally reports
peak tick lag. On Rocky Linux 9.9 ARM64, the live verification exercised all
48 stations with the packaged BW500 modem, recorded 91 attempts and 67
overlaps, and drained with 104 ms peak tick lag. Field qualification of the
path predictions requires held-out, matched BW500 captures.

Score a frozen field trial with `python tools/hf-live/evaluate_path_forecast.py
manifest.json`. The manifest declares schema 1, a common receiver mode, filter
and noise bandwidth in `profile`, SHA-256 digests for the `band_hour`, `p533`,
and `weather` model arms in `model_sha256`, and the training dates or receiver
IDs in `holdout`. Each row in `records` identifies the transmitter, receiver,
transmission ID, UTC time, band, and profile. Confirmed transmissions with
eligible captures supply an exact-payload outcome, an SNR measurement or
censoring status, and predictions from every arm. The evaluator reports
paired Brier scores, measured-SNR error and within-path best-band results.
[Field protocol](../../rust/docs/kiwisdr-test-plan.md)

See the [simulation design](../../docs/architecture/rf-agent-simulation-design.md)
for the station-policy evaluation goals and the [RF globe replay](../../globe/README.md)
for the separate stored Rust simulation traces.
