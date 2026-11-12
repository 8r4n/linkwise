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
| `modem::runtime` / `runtime::device` | Single-owner establishment/ARQ runtime, bounded bytes/events, token-bound presentation/release, capture continuity, expiry and simulated watchdog | Deterministic integration tests at the validated-event boundary; foreground `linkwise-modemd --simulate`; no operational PHY or TCP listeners |
| `api::CommandDecoder` / `host` | Bounded BW500/OFF command codec, command/notification stream transport, raw bidirectional DATA transport and channel-loss callbacks | Fragment/coalescing, resynchronization, binary byte preservation and EOF cleanup over duplex streams; operational modem backend not yet connected |
| `modem::receiver` | Streaming 48 kHz mono level 1–3 reception, absolute sample positions, bounded history | Chunk invariance, history wrap, EOF, overflow, non-finite input and nonframe tests; reference WAV replay |
| `audio::device` | Optional CPAL input, explicit device/channel, f32/i16/u16 conversion, bounded queue | Channel/conversion/overflow tests; built and enumerated input devices on macOS |

Receiver events contain burst candidates and CRC-valid DATA frames. They do not
indicate peer identity, connection establishment, payload delivery, or Winlink
message acceptance. DATA fields retain protocol padding and control bytes;
application-message reassembly is not implemented.

The WAV and device receive commands use a rolling pilot search for BW500
levels 1–3. It checks every 128 samples, normalizes pilot scores against
energy in the occupied band and guard bins, and tries frequency offsets of
0 and ±12 Hz. Because the search spans the expected frame length, a short
drop in energy does not split acquisition. CRC validation gates output and
overlap suppression prevents the same frame from being emitted again as the
search advances. The library opts in through
`Receiver::with_pilot_acquisition()`.

Level 4 still uses the fixed 1% RMS energy gate and decodes one frame per
burst when its measurement profile is available. The audio history is
bounded to 30 seconds. The WAV report counts overlong energy candidates in
`truncated_energy_candidates` while retaining the independent pilot results.
Neither acquisition path tracks sample-clock drift. A capture gap or
device failure ends the stream.


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
| Audio handshake | Linkwise and stock VARA HF 4.9.1 have established radio-free sessions at BW500, BW2300, and BW2750 | Setup repeatability, device playback, turnaround, and PTT ownership |
| DATA / ARQ | The virtual host supports measured frame layouts, keyed ACK-loss recovery, selected rate changes, turn handoff, and disconnect | Repeat qualification for recovery and integration with the native station runtime |
| Host interface | The radio-free bench exposes paired command and DATA sockets and a separate PCM connection | Managed station admission, device ownership, and operational session integration |
| Qualification | Exact mixed stock DATA transfers and bidirectional P2P mailbox delivery passed in the [interop bench](../../docs/testing/vara-interoperability.md) | Three consecutive passes per required case on one build and native OS, device, and RF qualification |

These stages are not all complete. In particular, parsing host commands or
returning a synthesized waveform must not produce an operational-modem claim.
The published layout notes explicitly leave the full DATA field's sequencing
and some recovery behavior unfixed; the current DATA codec therefore takes that
field as an input instead of inventing a rule.

## TCP bench and native playback

`linkwise-modemd --serve-bench W9SSJ` exercises the paired loopback TCP
listener with an explicitly simulated peer. The optional CPAL output
primitive provides timestamp estimates for a prepared burst on a named
device and channel. See the [runtime guide](modem-runtime.md) for entry
points and hardware qualification limits.

`control::Bw500IntermediateReply` generates and matches the called-keyed
eight-symbol reply family. Independent stock-derived vectors cover
same-level answers from both roles and a caller-issued decrease-one
answer. The NAK and other rate steps follow the published generator and
still need independent vectors. These codecs are available for future
session integration. The published frame notes are linked in the runtime
guide.

## Shared-core implementation plan (proposed)

The first milestone is now implemented at the validated-event boundary:
one serialized session owner, explicitly advanced monotonic time, bounded
byte/event queues, and simulated device execution. Run `cargo run --locked
--bin linkwise-modemd -- --simulate` from `rust/`. See the
[runtime guide](modem-runtime.md) for APIs, evidence, and remaining work.
The foreground simulation uses the clock-driven runner with simulated devices.

The authoritative proposed contracts and exit criteria are in the
[white paper modem chapter](../../docs/whitepaper/modem-software-architecture.tex)
and [software design, section 3.6](../../docs/architecture/software-design.md).
The dependency order is:

1. Session states, typed events, operation/session/transmission tokens,
   idempotent cleanup, finite budgets, and byte-accounting invariants.
2. Simulated capture continuity, playback presentation/cancellation, PTT
   assertion/release and turnaround. Inject faults and stale completions.
3. Complete the first BW500, compression-off link profile: wire-qualified
   handshake, packetization, DATA/reply/query, ARQ, duplicate handling,
   turn/rate behavior, keepalive and disconnect. A codec's supported level
   does not automatically become a supported session mode.
4. Add loopback TCP command/data listeners, exclusive pairing in either
   arrival order, attachment/stall timeouts, notifications and backpressure.
   Qualify each target client's initialization and buffer semantics.
5. Route standalone `CONNECT` and managed client reservations through the
   same station admission, lease and authorization path. Managed `CONNECT`
   consumes the reservation within the existing operation, once.
6. Demonstrate bidirectional reference-peer and named Winlink-client transfers;
   preserve command/audio/byte evidence and recovery results.

Independent reference measurements run alongside these stages wherever wire
behavior is unresolved. Socket tests and synthetic two-runtime transfers do
not establish on-air compatibility. Platform-specific audio/PTT adapters,
permissions and startup packaging follow the common contracts. Real device
and inhibited/conducted PTT qualification remain required before RF use.

The shared runtime must keep `host::ModemPort` (inbound client commands/data)
separate from proposed `ModemSessionPort` (backend lifecycle). Client channel
loss cancels the session and requests PTT release; replacement sockets do not
adopt it. No reconnect or process restart automatically replays unresolved
bytes. In-memory retention alone does not provide crash durability.

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
