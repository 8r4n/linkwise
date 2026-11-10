# Native modem implementation and validation

The Rust modem is a partially implemented BW500 level 1–4 receiver and protocol core.
It does not yet implement a complete VARA-compatible link or Winlink transport.

## Implemented boundaries

| Module | Implemented behavior | Validation |
| --- | --- | --- |
| `modem::bw500` | Level 1–3 DATA waveform synthesis, acquisition, soft decisions, turbo decoding and CRC validation | Synthetic audio round trips at all three levels; five published level 1/2 reference recordings |
| `modem::coding` | CRC, whitening, permutation PRNG and turbo FEC | CRC check vector and error correction tests |
| `modem::bw500_level4` / `bw500_waveform` | Level-4 channel coding, two-band DBPSK synthesis, pilot-based timing lock and CRC-checked reception | Setup and DATA audio round trips, streaming reception and corruption rejection; uses external measured phases/pulse |
| `modem::setup` | Caller identity frame encode/decode | Published setup vectors, SSID preservation |
| `modem::control` | Callsign-keyed request/response/confirm tones, connected-ack synthesis | Published call and ACK vectors, callsign discrimination |
| `modem::handshake` / `audio_handshake` | Establishment state machine and audio codec boundary; phase-specific peer-keyed matching | Two-role event tests and complete local synthesized-audio handshake through confirm; playback completion is supplied by the device owner |
| `modem::arq` | Bounded stop-and-wait sender, missing-answer queries, decoded-NAK retries, watchdogs and retained unresolved bytes | Event-level tests; not bound to a live PHY/session |
| `modem::data` | Uncompressed full/short DATA containers, caller-keyed trailers and CRC | Documented 200-byte delivery reconstruction, corruption/wrong-session rejection; changing full-frame fields remain caller-supplied |
| `api::CommandDecoder` / `host` | Bounded BW500/OFF command codec, command/notification stream transport, raw bidirectional DATA transport and channel-loss callbacks | Fragment/coalescing, resynchronization, binary byte preservation and EOF cleanup over duplex streams; operational modem backend not yet connected |
| `modem::receiver` | Streaming 48 kHz mono level 1–3 reception, absolute sample positions, bounded history | Chunk invariance, history wrap, EOF, overflow, non-finite input and nonframe tests; reference WAV replay |
| `audio::device` | Optional CPAL input, explicit device/channel, f32/i16/u16 conversion, bounded queue | Channel/conversion/overflow tests; built and enumerated input devices on macOS |

Receiver events contain burst candidates and CRC-valid DATA frames. They do not
indicate peer identity, connection establishment, payload delivery, or Winlink
message acceptance. DATA fields retain protocol padding and control bytes;
application-message reassembly is not implemented.

The energy detector uses a fixed 1% full-scale RMS threshold. It is intended for
controlled audio and has no AGC, fading recovery, or frequency/sample-clock
tracking. The level-4 implementation currently decodes one frame per burst and synthesizes no randomized acquisition lead. It does not implement carrier or sample-clock tracking. The receiver retains at most 30 seconds of audio. Candidates older
than retained history are marked `truncated` and produce no decoded frames.
WAV decoding reports an error for such candidates. Device capture fails on a
queue overflow or device error rather than joining discontinuous samples.

## Planned remote RF observation

The [KiwiSDR modem test plan](kiwisdr-test-plan.md) defines proposed capture,
conversion, frame-scoring, and regression experiments using cooperating remote
receivers. It is a draft plan, not evidence of RF tests or live interoperability.

## Reproduce reference validation

From the repository root:

```sh
python3 tools/test-modem.py
python3 tools/test-modem.py --audio-device
```

The runner downloads five WAV excerpts and their provenance from Saul St John's
[hfmodem repository](https://github.com/sstjohn/hfmodem/tree/952f79245240132f4af209afbad1f4b2ba256613/packages/hfmodem/hfmodem/tests/kestrel/fixtures/bw500-low-levels),
pinned to commit `952f79245240132f4af209afbad1f4b2ba256613`. It verifies each
SHA-256 before use and caches them under the ignored
`reference/vara/fixtures/bw500/` directory. No upstream implementation code or
recordings are redistributed by this change. The upstream provenance names
the stock-peer cable captures, expected bytes, and source hashes.

The runner also invokes `fetch-modem-profile.py` to cache checksum-pinned public
phase and pulse measurements. It extracts numeric literals with Python AST parsing
without executing downloaded source. The upstream data and generated profile are
kept in the ignored `reference/vara/fixtures/level4/` directory with attribution;
no measurement tables are embedded in the Rust source. The measured-data source
identifies its license as AGPL-3.0-only.

The command runs all tests, including the otherwise ignored external-audio
replay and level-4/audio-handshake tests. A missing or changed fixture fails the run. A normal `cargo test`
continues to work offline and explicitly reports the external replay test as
ignored. Cached, verified fixtures require no network access.

The reference recordings cover levels 1 and 2 only, including two frames in one
burst. Levels 3 and 4 currently have synthetic round-trip coverage, not recorded-peer
qualification. The complete two-role audio handshake is also synthetic. These are receive tests, not proof that stock VARA accepts our
transmitted waveforms.

## Receive from an audio device

From `rust/`:

```sh
cargo run --locked --features audio-device -- audio-inputs
LINKWISE_AUDIO_INPUT='Exact device name' LINKWISE_AUDIO_CHANNEL=0 \
  cargo run --locked --features audio-device -- receive-audio 60
```

The duration is 1–3600 seconds. Input must support native 48 kHz with the selected
zero-based channel. An omitted name selects the system default input. To enable level 4, set `LINKWISE_BW500_PROFILE` to the generated measurement
profile. Without it the receiver handles levels 1–3. The CLI
emits JSON lines after each completed burst, containing sample ranges, the
truncation flag, level, control byte and payload hex. It captures input only.
It does not key PTT, play audio, or establish a link. Microphone permission may
be required by the operating system. Linux device builds require ALSA development
headers; the optional feature keeps those dependencies out of the default build.

Offline replay uses the same streaming receive worker:

```sh
cargo run --locked -- decode-wav ../reference/vara/fixtures/bw500/bw500-level1-arm1.wav
```

## File-only handshake stimuli

`render-wav` produces bench files without playing them or keying PTT:

```sh
# request.json contains: {"kind":"request","called":"W1AW"}
cargo run --locked -- render-wav request.json request.wav
```

Supported kinds are `request` / `confirm` with `called`, and `setup` / `ack`
with `caller`. Setup requires `LINKWISE_BW500_PROFILE`. Output is 48 kHz mono
float WAV. The setup waveform has no synthesized random acquisition lead.

## Stage status and remaining integration

| Stage | Status | Remaining gate |
| --- | --- | --- |
| Level-4 waveform | Native single-frame encoder/receiver implemented; local audio tests pass | Independent stock recordings, acquisition lead and multi-frame behavior |
| Audio handshake | Two local instances establish through decoded audio | Device playback, turnaround, PTT ownership and live reference-peer acceptance |
| DATA / ARQ | Container codec and bounded event-driven sender implemented | Wire field sequencing, reply/query codecs, duplicate handling, rate/turn/keepalive/disconnect integration |
| Host interface | Command codec and command/data stream transports implemented | Socket ownership/pairing and connection to an operational session engine |
| Qualification | Reference level 1/2 replay and synthetic level 1–4 tests pass | Bidirectional unmodified-VARA transfers, impaired-link tests and native OS/device qualification |

These stages are not all complete. In particular, parsing host commands or
returning a synthesized waveform must not produce an operational-modem claim.
The published layout notes explicitly leave the full DATA field's sequencing
and some recovery behavior unfixed; the current DATA codec therefore takes that
field as an input instead of inventing a rule.

## Reference setup attempted on this workstation

The user has no configured reference instance. A temporary, isolated Docker
bench was built on the ARM64 Mac using pinned i386 Debian bookworm, Wine 8,
Xvfb and PulseAudio, mounting the already obtained VARA 4.9.0 files read-only.
The Microsoft VB6 SP6 runtime was downloaded through the archive URL used by
Winetricks 20250102 and verified against SHA-256
`467b5a10c369865f2021d379fc0933cb382146b702bbca4bcb703fc86f4322bb`.

Wine initialized, but COM registration repeatedly failed under qemu-i386 with
`Unhandled illegal instruction at address 7BC57F86`, followed by WineDbg error 5.
The error dialog was visually verified. No functioning VARA control endpoint or
audio-loopback reference resulted. This is a failed environment attempt, not a
modem interoperability failure or a qualification result. A functioning reference
host is still required to close the measurement and qualification gates. The
experimental reference container was stopped after diagnosis.

The published [protocol notes](https://github.com/sstjohn/hfmodem/tree/952f79245240132f4af209afbad1f4b2ba256613/docs/protocols/vara)
are the external specification used to identify these boundaries. Event-level
or synthetic self-tests must not be promoted to interoperability claims.
