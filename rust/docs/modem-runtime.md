# Shared modem runtime

`modem::runtime::Runtime<D>` is the portable session owner. It integrates the
existing level-4 establishment controller and stop-and-wait ARQ sender at the
**validated protocol-event boundary**. It is the first shared-core milestone,
not an operational VARA or Winlink modem.

Use the [station setup guide](modem-setup.md) to check configuration,
native audio access, and local modem port availability.

## Run the foreground simulation

From `rust/`:

```sh
cargo run --locked --bin linkwise-modemd -- --simulate
```

The executable establishes caller and responder sessions, checks a 256-byte
binary DATA intent, deliberately loses its first answer, requests peer state,
and retires the bytes once after a synthetic delivered answer. It reports a
snapshot and exits. The scenario uses simulated protocol and device events.
The runner's timer schedules the state query while the client waits.

## Run the BW500 audio loopback

From `rust/`:

```sh
profile=$(python3 ../tools/fetch-modem-profile.py)
cargo run --locked --bin linkwise-modemd -- --simulate-audio "$profile"
```

Two runtime instances complete the BW500 establishment handshake through
48 kHz audio. Each takes a turn sending packetized binary data over the
existing level-4 waveform and CRC-protected full frames. The cable model
applies attenuation, sample offset and turnaround and advances a virtual
clock by the sample duration. One direction erases the initial answer and
recovers with a state query. The other erases DATA and retries after a decoded
negative response to its query. The bench replays a decoded frame in each
direction to check receive deduplication and compares the delivered streams
byte for byte.

The bench uses a private fixed-length DATA payload with a record kind,
sequence number and length. Query and answer records use that same test
format. This format is confined to the audio loopback. Production BW500
framing, answer/query handling and turn ownership still need independent
qualification. The pinned [frame notes][frames] describe role-dependent
BW500 replies and the [recovery evidence][recovery] documents the observed
stock BW2300 loss scenarios.

[frames]: https://github.com/sstjohn/hfmodem/blob/952f79245240132f4af209afbad1f4b2ba256613/docs/protocols/vara/04-frame-block-formats.md
[recovery]: https://github.com/sstjohn/hfmodem/blob/952f79245240132f4af209afbad1f4b2ba256613/docs/protocols/vara/21-data-answer-recovery.md

## Run the paired TCP bench

From `rust/`:

```sh
cargo run --locked --bin linkwise-modemd -- --serve-bench W9SSJ
```

The foreground bench listens on loopback ports 8300 (commands) and 8301
(binary DATA). Optional port arguments override the defaults. Port 0 lets
the OS select a free port. A single trusted local client can attach either
socket first. Both sockets must arrive within five seconds of the first.
An additional connection cannot replace either socket while a pair is active.
A stalled socket write has a five-second deadline. Losing either channel
closes the other and awaits session cleanup.
The paired command socket sends `IAMALIVE` every 60 seconds.

`VERSION` identifies the simulated bench. `CONNECT W9SSJ W1AW` runs a
simulated handshake, and the peer echoes each acknowledged binary block.
`ABORT` and `DISCONNECT` end the simulated session. The callsign supplied on
the command line is the allowed local identity. The two sockets carry no
pairing credential, so deploy this harness only on a trusted loopback host.

`P2P SESSION` is accepted while idle for the simulated peer. The bench
returns `WRONG` for gateway sessions, incoming listen requests, public
mode and CWID settings, enabled compression, and wider bandwidths.

## Serve a radio-free BW500 session

`--serve-virtual-bw500` connects the paired host sockets to Linkwise's BW500
handshake, transmit-turn control, and uncompressed DATA waveforms. It uses
the same measured level-4 profile as the audio bench. Start it from `rust/`:

```sh
profile=$(python3 ../tools/fetch-modem-profile.py)
cargo run --locked --features audio-device --bin linkwise-modemd -- \
  --serve-virtual-bw500 W1AW "$profile" 8300 8301 8302
```

The command, DATA, and audio sockets bind to IPv4 loopback. Attach an audio
peer and the two host sockets before calling or enabling `LISTEN ON`.
Only one paired host and one cable connection are served at a time. The
audio socket carries one burst per record. Each record is a four-byte
big-endian sample count followed by that many little-endian IEEE float
samples at 48 kHz mono. The limit is 288,000 samples per record. Route each
burst to the peer before returning its response. Use a trusted isolated
test environment for the unauthenticated sockets.

With the audio feature, a second process can attach the cable to a pair of
named virtual audio devices:

```sh
linkwise-modemd --virtual-audio-device 8302 'Virtual input' 'Virtual output'
```

Use dedicated loopback devices with the input and output on independent
paths. The bridge plays and captures channel 0. The service does not
manage radio PTT.

Either station can queue up to 4,096 bytes. The sender coalesces TCP reads
for up to 100 ms and splits the stream into uncompressed deliveries of
up to 203 bytes using the measured BW500 frame layouts. The receiver
releases a delivery to its host after the closing frame. The stations
can hand over the transmit turn within one connection.

A repeated closing frame is acknowledged again without redelivering its
bytes. A rejection of the first DATA frame can trigger one lower-rate
retry using a measured layout. This covers short deliveries of up to
31 bytes and deliveries that fit five full level-3 frames and a close.
The five-frame retry delivered 200 bytes between Linkwise daemons. In
the stock VARA test, VARA rejected the first level-3 retry.
Ambiguous intermediate repeats fail the session. If the caller loses a
closing acknowledgment, it sends bounded state queries and retains the
bytes until the peer confirms delivery.
An unanswered query or missing intermediate acknowledgment ends the
connection with delivery unconfirmed. `ABORT` and `DISCONNECT` reset
the local state.

Run the socket and waveform regression from the repository root:

```sh
python3 tools/test-virtual-bw500.py rust/target/debug/linkwise-modemd \
  reference/vara/fixtures/level4/bw500-level4.json
```

Pat 1.0.1 completed a Winlink B2F peer-to-peer message transfer between
two Linkwise daemons over both the socket cable and Ubuntu's native ALSA
loopback. Stock VARA HF 4.9.1 transferred 200-byte test streams in both
connection roles in separate sessions. A shorter test exchanged 13 bytes
from Linkwise and nine from VARA within a single connection.
Winlink Express 1.8.6.0 also received and displayed a B2F peer-to-peer
test message from Pat through two Linkwise daemons over the socket cable.
The stock VARA bench verified caller recovery after dropping a closing
acknowledgment.

## Serve a wide-mode bench session

Use one fixed bandwidth at each end of the isolated audio cable:

```sh
linkwise-modemd --serve-virtual-wide W1AW 2300 8300 8301 8302
```

Use `2300` or `2750`. The same command, DATA, and audio socket protocol
applies. The wide modes use level-4 setup and uncompressed closing DATA
frames carrying up to 86 bytes each. Longer host writes are split across
deliveries. Stock VARA HF 4.9.1 exchanged exact 13-byte payloads in both
connection roles at each bandwidth. Wide adaptive rates and multi-frame
deliveries require additional session support. Add `--wide` to the socket
regression command to exercise both wide modes between Linkwise peers.

## Native output primitive

With the `audio-device` feature, `linkwise audio-outputs` lists output
devices. `audio::playback::Playback` submits one bounded 48 kHz mono burst
to an explicitly named output device and channel. The callback emits silence
on other channels and reports a completion estimate based on CPAL's
presentation timestamp. The runtime still needs hardware evidence before
it can use this estimate for radio turnaround or PTT release.

## Ownership and lifecycle

`runner::spawn` owns one `Runtime` in a Tokio task and exposes a cloneable
command handle. It drives device polling and protocol deadlines from a
monotonic clock at 5 ms intervals. The normal command queue returns
`RequestError::Busy` at capacity. Device observations and abort use a separate
bounded priority queue. Replies use one-shot channels so the runner can
continue even if the caller stops consuming them. Device methods must return
promptly.

Use `Handle::now` to set a lease expiry relative to the runner's epoch.
Dropping all handles faults a live session and requests release while
preserving retained bytes. The join handle returns the runtime after release
evidence or one transmit timeout. Inspect `release_pending` before reusing
the device. Direct `Runtime` callers can still supply virtual time explicitly
for deterministic tests.

`begin` accepts an owner ID, operation ID, local/optional remote callsign and
lease expiry. It generates a fresh session ID. No remote callsign explicitly
admits a listening/responder session. `Admission` is an internal correlation
record: it does not validate station authority, grant a lease, or authorize
real keying. The future station service must supply those checks.

States are idle, establishing, connected, faulted and aborted. Connection
occurs only after the existing handshake has completed its confirm exchange.
Snapshots then report the actual peer. Normal closing/completed states and
wire disconnect/keepalive behavior belong to the later operational profile.

`enqueue` accepts nonempty, already packetized blocks only while connected.
It validates owner/session, applies a finite byte limit and refuses atomically,
returning the original bytes. It is not yet a raw-host stream packetizer.
A second admission cannot replace a live session, unconfirmed transmission,
unconsumed events, or retained unacknowledged bytes.

## Device execution and timing

`ExecutionDevice` accepts one owned transmission intent and a release deadline.
Intents are establishment bursts, DATA blocks or state queries. A future PHY
adapter must encode/decode the real waveforms and qualify reply/query behavior;
the runtime never fabricates wire fields or ACK recognition.

Each transmission carries session identity and a monotonically increasing
local sequence. Presentation requests release. Only matching presentation
followed by observed release/turnaround is passed to handshake/ARQ as successful
transmission. Stale, duplicate, wrong-session and post-abort callbacks cannot
advance the protocol. Deadline checks precede late presentation/completion;
already observed device failure/watchdog release has priority over protocol
timeouts.

`SimulatedDevice` has a single owned intent slot and an independent virtual-time
watchdog. Its watchdog can be polled and release simulated keying without
running the session owner. This does not implement a real-time thread or a
physical PTT watchdog. A future native adapter must enforce release independently
of the runner, even if it stops polling.

`capture_range` checks half-open absolute sample ranges within one capture
epoch. Gaps, overlaps, malformed ranges or changed epochs fault the session;
`capture_discontinuity` reports device/capture failure directly. Neither method
decodes audio. Stream restart requires a new session.

Lease, total-session, handshake, transmit and answer deadlines use the same
monotonic epoch. Backward time is rejected. Missing answers query peer state;
only validated negative answers retry DATA. Retries and queries are bounded.

## Accounting, pressure and cleanup

Snapshots preserve this invariant:

```text
accepted = pending + outstanding + acknowledged + unresolved + discarded
```

Retry attempts are separate from unique accepted/acknowledged bytes. A fault
moves the outstanding block to unresolved; untouched queued blocks remain
pending. Payload remains in bounded ARQ memory, accessible only through the
owner/session-checked `retained_blocks` inspection API. Metadata events contain
no payload. Retention is not crash durability or permission to replay.

The finite event queue is drained with `pop_event`. Overflow faults the live
session and requests device release before event publication. The authoritative
snapshot retains the terminal reason even when the queue has no room for a
fault event. Abort/release bypass that queue; repeated abort is idempotent.
`abort` discards retained blocks with explicit accounting and requests release.
The paired TCP bench calls it during channel-loss cleanup.
An uncertain release keeps admission blocked until matching release/watchdog
evidence arrives. A new session never adopts old retained payload.

## Stock VARA BW500 delivery tests

The Windows VARA HF 4.9.1 bench uses a virtual cable and native Linkwise
waveform peers. It exercises separate sessions with each modem as caller.
The multi-frame fixtures cover level-4 DATA, a level-3 closing frame on
the Linkwise transmit path, and stock VARA's level fallback after a
negative acknowledgment. The DATA parser accepts the observed
`0x42` and `0x82` short closing fields after CRC validation.

Replay the local capture set from `rust/`:

```sh
LINKWISE_STOCK_BW500_CAPTURE_DIR=../reference/vara/fixtures/stock-multi-2026-11-11 \
cargo test --locked --release --test bw500_replay \
  decodes_stock_bw500_delivery_and_level_fallback -- --ignored
```

The [native audio loopback test](../../docs/testing/utm-test-environments.md#verify-playback-through-a-native-loopback)
covers 48 kHz playback and capture on the Linux guest audio backends.
The audio bridge and the native-device test exercise separate paths.
The daemon offers a separate radio-free waveform service. The original
paired TCP echo bench continues to use its simulated backend.

## Verification and remaining work

`tests/modem_runtime.rs` covers two-role establishment, binary preservation,
byte conservation, delayed/duplicate callbacks, backpressure, competing owners,
answer loss, negative retries, query exhaustion, capture discontinuity, device
release failure, watchdogs, deadlines, event overflow, abort and new-session
isolation. Run:

```sh
cargo test --locked --test modem_runtime
cargo clippy --locked --all-targets -- -D warnings
```

Remaining work includes native waveform/device integration,
production DATA packetization and wire reply/query qualification, host receive delivery and
on-air duplicate suppression, turn/rate/keepalive/disconnect behavior, an operational backend for the paired TCP
listener, station admission/grants, durable operation records,
reference-peer/client interoperability, and OS-specific adapters/packaging.
