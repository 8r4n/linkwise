# Rust modem lab with SigPlot

From the repository root:

```sh
cd rust
cargo run --locked --features demo --bin modem-demo
```

Open **http://127.0.0.1:8787**. The first run automatically encodes and decodes
`HELLO FROM LINKWISE` at BW500 level 3. Select a level, enter a message, and
press **Run modem**. **Stream at audio speed** is enabled by default: the Rust
server feeds 1024-sample chunks into the receiver on a 48 kHz sample clock and
sends plot updates roughly every 107 ms. Watch the waterfall and waveform grow,
the latest spectrum change, and CRC-valid payloads appear as the receiver emits
them. Frame acquisition can delay output until an audio candidate closes; an
empty payload while receiving does not indicate a failed stream. Final byte
verification follows the receiver flush. **Stop stream** cancels the request and
leaves partial results unverified. Disable streaming for the original fast batch
run. Each stream is finite and uses the configured message or recording. Use `-- --port 8788` to choose another port. Stop with Ctrl+C.
The server binds only to localhost. No radio, audio device, browser extension,
JavaScript CDN, or VARA installation is required.

## What the demo runs

All signal processing and verification happen in Rust. The browser renders the
results using bundled [SigPlot 3.1.7](https://sigplot.lgsinnovations.com/).

1. Split UTF-8 message bytes into supported full frames, padding the last frame.
   Levels 1, 2, and 3 carry 9, 22, and 34 message bytes per frame, respectively;
   the demo permits up to three frames. Level 4 carries 43 bytes in one frame.
2. Encode and synthesize audio with the native modem implementation at 48 kHz.
3. Optionally apply reproducible Gaussian noise or a middle-third dropout.
4. Feed 1024-sample chunks through the actual streaming Rust receiver and flush it.
5. Compare CRC-valid decoded frames, their level/control field, and every payload
   byte with the expected input. Missing or mismatched frames remain unverified.
6. Compute a 2048-point Hann-window STFT with a 1024-sample hop in RustFFT.
   SigPlot displays the 0–3 kHz waterfall, min/max waveform envelope, and a
   selectable spectrum slice. The spectrum uses coherent-amplitude normalization
   in dBFS; it is not a power spectral density. The waveform envelope retains
   extrema in blocks of 512 samples rather than plotting every audio sample.

Use **Dark mode** in the header to switch themes. The demo follows your system
appearance until you choose a theme, then remembers your choice in this browser.

The time slider selects an FFT window; audio playback also moves the spectrum
slice. Download the received waveform as a mono 48 kHz float WAV. The evidence
panel lists frame bytes, verification results, and measured processing stages.
Streaming processing times include pacing and client backpressure. Only completed
runs produce an audio download. Audio downloads are retained for the last four runs in memory.

Noise uses a fixed seed for repeatability. Displayed SNR is measured before any
dropout, over the active audio region. Low SNR can defeat acquisition; this is
shown as incomplete verification rather than a successful connection.

## Optional level 4 and stock recording replay

From the repository root, fetch the existing pinned external inputs:

```sh
python3 tools/fetch-modem-profile.py
python3 tools/test-modem.py
```

Restart the server after fetching the level-4 measurement profile. Reload the
page after fetching recordings, then select **Stock recording** and decode one
of the five published captures. Each original WAV is checked against its pinned
SHA-256 before decoding. Selected noise/dropout is applied after that check.
The source hash remains visible in the evidence panel.

By default inputs are found below `reference/vara/fixtures/` in this checkout.
`LINKWISE_BW500_PROFILE` can select another profile JSON and
`LINKWISE_BW500_FIXTURES` can select the directory containing the named capture
WAVs. Recordings must still match the pinned hashes.

## Scope and verification

This demonstrates native BW500 codec loopback and receive-only decoding of
recorded stock signals. It does not establish a live VARA, RF, ARQ, or Winlink
session. The demo knows the expected frame schedule and original message length
for comparison; padding removal is not evidence of live protocol reassembly.
See [modem implementation status](modem.md) for qualification limits.

```sh
cd rust
cargo test --locked --features demo
cargo clippy --locked --all-targets --all-features -- -D warnings
```

Demo tests exercise real level 1–3 round trips, deterministic noise, dropout
failure reporting, input validation, WAV export, and the FFT frequency/amplitude
axis using a known tone. Optional external capture/profile tests are documented
in the modem guide. The HTTP API additionally exposes `GET /api/config`,
`POST /api/run`, `POST /api/stream` (bounded NDJSON), and `GET /api/runs/{id}/audio.wav`. DSP requests are bounded to
two concurrent workers shared by batch and streaming requests, 8192-byte request bodies, and one million audio samples.

## Frontend maintenance

The checked-in bundle means Cargo builds need no Node installation. To rebuild
SigPlot after changing the pinned dependency:

```sh
cd rust/demo-ui
npm ci
npm run build
```

Keep `package-lock.json`, the generated bundle, its linked legal notices, and
`public/vendor/LICENSE.sigplot` together. SigPlot is Apache-2.0 licensed; its
upstream NOTICE is included as `public/vendor/NOTICE.sigplot`. UI sources are
`public/index.html`, `public/style.css`, and `public/app.js`; they are embedded in
the Rust executable, so restart/rebuild it after editing them.
