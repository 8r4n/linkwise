# Modem demo media

`modem-streaming.gif` is a cropped screen recording excerpt of the actual local
SigPlot demo in Google Chrome, captured on 2026-11-09. It shows a BW500 level-1
native stream carrying `HELLO FROM LINKWISE` across three frames, starting partway
through reception and ending with 3/3 exact payload verification. Frames were
sampled approximately every two seconds; the final result is held longer. Browser
tabs, address bar, and bookmarks are cropped out. The GIF has no audio.
`modem-demo.png` is the final captured frame.

The WAV files are synthesized by the Rust modem through `/api/run` and exported
through `/api/runs/{id}/audio.wav`. Each waveform's original float32 samples were
decoded and compared with the input by the Rust demo before conversion to portable
mono 48 kHz PCM16. `audio-manifest.json` records requests, decoded frame bytes,
durations, and SHA-256 hashes of the distributed PCM16 files. Each distributed PCM16 file was also independently decoded with the Rust
`decode-wav` CLI, with frame bytes compared against the float32 run; the manifest
records this as `pcm16_decode_verified`. Seed 20261109 makes
the 24 dB SNR noise reproducible. These are native codec demonstrations, not
recordings of stock VARA software, live ARQ sessions, Winlink deliveries, or RF.

To regenerate the audio from the repository root:

```sh
# In one terminal:
cd rust
cargo run --locked --features demo --bin modem-demo

# In another terminal, from the repository root:
python3 tools/export-demo-audio.py
```

To reproduce the GIF, open http://127.0.0.1:8787, select native loopback, level 1,
`HELLO FROM LINKWISE`, streaming enabled, noise/dropout disabled, and dark mode.
Click **Run modem** and record the plots through final verification. Crop to the
page and export a looping GIF. To decode an exported example independently:

```sh
cargo run --locked --manifest-path rust/Cargo.toml --bin linkwise -- \
  decode-wav docs/media/modem/bw500-level1-clean.wav
```

The media has the same license as the repository. SigPlot's applicable license
and notices remain in `rust/demo-ui/public/vendor/`.
