# Shared modem runtime

`modem::runtime::Runtime<D>` is the portable session owner. It integrates the
existing level-4 establishment controller and stop-and-wait ARQ sender at the
**validated protocol-event boundary**. It is the first shared-core milestone,
not an operational VARA or Winlink modem.

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

`VERSION` identifies the simulated bench. `CONNECT W9SSJ W1AW` runs a
simulated handshake, and the peer echoes each acknowledged binary block.
`ABORT` and `DISCONNECT` end the simulated session. The callsign supplied on
the command line is the allowed local identity. The two sockets carry no
pairing credential, so deploy this harness only on a trusted loopback host.

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
