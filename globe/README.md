# Linkwise RF globe

Replay the Rust network simulator on an interactive Cesium globe. A mint arrow shows a successful transfer in the current slot. A dashed orange line shows a failed call. A gold curve shows the selected RF circuit. Each network hop ends at a station. A single circuit can contain several skywave hops.

## Run locally

From the repository root, run the following, then open **http://127.0.0.1:8743** in a browser with WebGL 2. The first command downloads the pinned CesiumJS 1.130.0 distribution and validates its SHA-512 digest. All subsequent playback uses local imagery, software, forecasts and weather data. The viewer uses JavaScript ES modules directly.

```sh
python3 globe/prepare.py
python3 -m http.server 8743 --bind 127.0.0.1 --directory globe
```

## Follow a message

The default view opens on six published European VARA HF gateway locations. Select **Synthetic · North Atlantic** to reproduce the original six-node fixture. That fixture opens at slot 6. Message M1 has arrived at D, and D is attempting the circuit to E. A transfer shown in slot *s* commits at boundary *s + 1*. Under adaptive routing the message reaches F at slot 10. Switch to fixed routing and seek to slot 20 to see expiry at C. The message inspector lists its actual call attempts. Select an attempt to seek directly to the call. Select a map station to inspect its queue and radio activity. Use **Overview** to return from the tilted circuit view.

Choose a global replay to see simultaneous calls and failures. The 100- and 1,000-node runs cover 48 hours in 15-minute slots. Both use the simulator's six regional gateways with deterministically offset synthetic access stations. The regional fixture uses one-minute slots and six illustrative Atlantic locations. Geographic assignments and channel frequencies are scenario assumptions.

## Inspect propagation

Select **Refracted F2 ray** and **Inspect circuit**. At slot 6 the 7.10 MHz Madeira–Azores circuit has a connecting modeled ray. Inspecting 14.10 MHz at the same instant changes the circuit forecast and the ray solution. The frequency selector explores a counterfactual and preserves the recorded traffic.

- **Ionospheric conditions.** The archive has 194 timestamped global grids from October 20–21, 2026. The viewer loads the latest grid at or before the cursor within a ten-minute interval. Gray areas or missing intervals have no usable value. The layers give vertical TEC in TECU, F2 peak altitude in km, and the source observation-coverage flag. Flag zero can represent a background-model estimate. Each frame's manifest records the original URL and SHA-256. Grid cells are rounded for display. [NOAA GloTEC](https://www.spaceweather.gov/products/glotec)
- **Vertical section.** The gold dashed profile samples GloTEC hmF2 along the ground track. Blue shading is a modeled Chapman F2 density profile normalized to its local peak, using a 50 km scale height. It is an assumed vertical shape. The archive includes TEC, hmF2 and NmF2 and has no measured vertical density slices.
- **RF geometry.** Schematic mode draws an illustrative path. Ray mode integrates a spherical, isotropic cold-plasma Snell invariant using midpoint hmF2 and NmF2 for each assumed hop. It searches launch elevations from 1° to 89° and accepts a range residual below 1 km per hop. A missing grid or unsuccessful search leaves the ray absent. The solver approximates a horizontally uniform F2 layer within each hop, excluding magnetic splitting, D/E layers, lateral gradients and absorption. It has numerical consistency checks. Independent ray-tracer and field validation remain open.
- **Circuit performance.** The archived VOACAPL Method 30 calculation covers all six regional links and the global gateway ring, four frequencies and all 24 UTC hours. Inputs are October 2026, illustrative smoothed sunspot number 83, 100 W, short path, two isotropic antennas and a required 39 dB·Hz S/N₀. The inspector distinguishes monthly VOACAP reliability, median S/N₀, received power and dominant mode from instantaneous GloTEC fields. Access stations have schematic/ray inspection and no archived VOACAP prediction.
- **Routing experiment.** The optional 90% reliability requirement recomputes both policies in Rust with below-threshold gateway contacts excluded from the commissioned schedule. Access contacts retain the baseline schedule. All policies use seed 17 and common offered traffic and outages. This strict setting removes all gateway opportunities used by these schedules. It illustrates sensitivity to a planning threshold. It does not estimate measured modem delivery probability.

## Reproduce the evidence

```sh
python3 globe/prepare.py --replays
node globe/check.mjs
cargo test --locked --manifest-path rust/network/Cargo.toml
cargo clippy --locked --manifest-path rust/network/Cargo.toml --all-targets -- -D warnings
```

The replay export is also available as `linkwise-network replay six 17 [forecast.json]` or `replay global-N 17 [forecast.json]`. Its records contain the slot, message ID, original contact index, source station and outcome. The existing simulator command keeps its original output.

The offline check reconstructs traffic across all 16 runs and verifies per-station half-duplex constraints, missed calls, final delivery totals, the scripted route, grid dimensions and a reference circuit. Under the baseline schedule the fixed/adaptive delivery counts were 0/1 at six nodes, 2/78 at 100 nodes and 0/2 at 1,000 nodes.

`weather.py` recaptures the original NOAA files while they are available. `forecast.py` regenerates the model table with the local `linkwise-voacap:local` image. The captured VOACAPL revision was `c12a98b348c39f36f2659b8b3aa97b86284c8a1d` and the Docker image ID was `sha256:6591a61b8d8bf61877a852fbd79e7c92a0447851a753618537cd68b6a591a667`. Neither recapture step is needed for playback.

The browser checkout exercised forward/backward seeking, both policies, frequency changes, the ray solver, the tilted view, forecast gating, archive gaps and the 1,000-node load. It issued zero external requests. On an Apple M4 in headless Chrome with SwiftShader at 1280 × 800, the 1,000-node replay with scheduled contacts and weather enabled averaged roughly 16 browser animation callbacks per second while repeatedly advancing the slot. This misses the proposed 30 fps target in that software-rendered configuration. Interactive hardware-GPU frame rate and physical HF accuracy remain unverified.

With the server running, use `CHROME_BIN=/path/to/chrome node globe/browser-check.cjs` to repeat the browser checks and timing sample. The check requires Node with `playwright-core` available. Set `GLOBE_URL` to test a different local port.

## Orbital presentation

Choose **Orbital tour** in the globe or open [the local tour](http://127.0.0.1:8743/?orbital). Play or scrub the 44-second sequence to see the Iberian night coast, three simulated European transfers, a modeled F2 ray between F5ZFX and HB9T, and the worldwide directory replay with NOAA vertical TEC. The moving point traces a modeled link at presentation speed. Each shot reads the same simulator events and circuit inspector as the interactive globe.

The daylight surface combines Terra and Aqua MODIS corrected-reflectance mosaics for October 20, 2026. Blue Marble fills remaining gaps. Night lights come from the 2016 Black Marble VIIRS composite, with a low-light rendering of the daytime cloud and terrain context. These composites provide geographic context across the replay. Individual clouds and lights are not timestamped observations at the simulated contact. Dynamic day/night blending follows the Cesium clock. [NASA GIBS imagery](https://gibs.earthdata.nasa.gov/) · [NASA Black Marble source and credits](https://science.nasa.gov/earth/earth-observatory/earth-at-night/maps/)


## Published VARA HF locations

The default globe and orbital video use the November 15, 2026 Winlink directory snapshot. The worldwide scene has 191 distinct gateway callsigns. Each included gateway reported that day and listed at least one public VARA HF channel (mode 50, 2–30 MHz). Coordinates and center frequencies are copied from the directory. Select a station to inspect its callsign. The curated route goes from EA1URO in Spain through F5ZFX in France and HB9T in Switzerland to LA1B in Norway. [Winlink public gateway map and directory](https://winlink.org/rmschannels)

The contact schedule, traffic, and routing decisions are simulated. For each candidate contact the scheduled frequency is the advertised center frequency of the receiving gateway closest to 7.1 MHz. The European circuit comparison uses HB9T's listed 3.6015 and 14.0885 MHz channels. The weather replay uses the separately dated October archive. No gateway was contacted. No synthetic-fixture VOACAP forecast is reused for an actual gateway pair.

To regenerate, build the Rust network CLI and run `python3 globe/vara.py /path/to/rms.json` using the archived directory response (SHA-256 `c0e259626bc2ef3342a17e24af5d7a062d5b1b77a19fd30b9236260e0ed41f3b`). The generator submits each schedule to the Rust simulator with seed 17 and saves both policy results.
