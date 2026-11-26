# Station integration status

The station service imports and ranks RMS channels and prepares a simulated
radio under a short-lived lease and grant. With the managed BW500 profile, the
station owns two one-shot waveform modem processes and both B2F client
attachments for a single attempt. It journals peer connection, verified fixture
delivery, and local cleanup. The live station release still requires a native
device and client binding, hardware transmit qualification, and a Winlink
gateway exchange.

## Production baseline

The production target is the Rocky Linux 9 ARM64 server RPM built with
`audio-device,rocky-mail-tls`. That build uses ALSA for station audio, systemd
for the modem and mail services, and Rocky's system OpenSSL provider for mail
TLS. The Rocky server candidate uses the `1.rockymail` RPM release suffix.
The receive-only Rocky assessment RPM and the stock Windows VARA reference
bench each supply separate qualification evidence. [Rocky server guide](../install/rocky-server.md) · [Rocky RPM workflow](../../.github/workflows/rocky-server.yml)

Collect the build commit, RPM checksum, installed binary versions, and
managed BW500 smoke result on Rocky ARM64. The installed-binary smoke uses a
private MCP profile and virtual audio. Native ALSA, systemd and SELinux on the
commissioned host, signed upgrade and recovery, and the authorized live
gateway exchange have their own acceptance gates below.

## Implementation and evidence

| Requirements | Implemented and evidenced | Completion gate |
| --- | --- | --- |
| OVR-F-003–006, 021, 023, 034–036 | The station imports bounded, versioned inventory snapshots and optional VOACAP results from configured files. It records hashes and source times, distinguishes gateway and channel counts, and explains exclusions and the ranker's select, wait, or no-route result. The scheduler refreshes the inventory file independently of MCP calls. | Implement and qualify the authorized live RMS source adapter and a pinned VOACAP runner. Record per-path assumptions, model/run IDs, collection health and freshness. Add local observations, antenna fit and measured outcome history to the scoring model. [Station service](../../rust/src/olm.rs) · [Inventory and forecast inputs](../../rust/src/olm/inventory.rs) |
| OVR-F-002, 007–014, 018–020, 022, 033 | The managed BW500 operation binds its two host and waveform attachments to one attempt and lease. Command and DATA loss stop the corresponding modem process. The station checks both clients' B2F and teardown results, verifies the received fixture envelopes, and reaps both processes before releasing the lease. CMS and recipient stages remain unknown. | Bind the same lifecycle to a commissioned native modem and client. Record the live modem and gateway stages separately. The native modem needs measured playback, hardware safety release, and a qualified RMS/CMS exchange. [Station service](../../rust/src/olm.rs) · [Managed waveform and B2F fixture](../../rust/src/olm/virtual_bench.rs) · [MCP tools](../../rust/src/mcp.rs) |
| OVR-F-013, 020, 031 and OVR-Q-001, 009 | The paired command and DATA sockets detach on EOF, including while a backend reply is pending. `CONNECT` and immediate `DISCONNECT` acknowledge admission before waveform I/O. Decoded `ABORT` takes priority over pending waveform work. Forced service stop cancels local playback without sending a disconnect burst. An interrupted virtual record requires a new cable/host attachment. If native presentation is unconfirmed, the service closes the host listeners and waits for restart. | Use a single session owner for the native and virtual modem paths, with tokened device events, explicit cancellation and bounded byte accounting. Qualify buffering, channel loss, wrong owner, responder admission and abort under simultaneous activity. [TCP host](../../rust/src/host/tcp.rs) · [Waveform engine](../../rust/src/bin/linkwise-modemd/virtual_host.rs) · [Native audio](../../rust/src/bin/linkwise-modemd/station.rs) |
| OVR-F-025–030, 037–040 | Shadow scenarios exercise the station tool boundary with scripted timing and outcomes. Historic rankings remain queryable by ID after restart. The separate adaptive network bench provides seeded contact simulations and durable independent-station workflows. | Run the station tool contracts with a virtual-time RF and outcome model. Compare policies on paired held-out scenarios and publish denominators and uncertainty. Recompute historic rankings from their recorded inputs and validate the results independently. [Simulation design](rf-agent-simulation-design.md) · [Network bench](../../rust/network/README.md) |

## Managed radio-free milestone

One accepted grant starts two dedicated `linkwise-modemd` processes. Fresh
capabilities associate the command, DATA, and virtual PCM sockets, and each
daemon accepts one wire attempt. The same B2F code used by `linkwise mail`
runs as caller and listener. When one side finishes, its sockets remain
attached while the other side verifies its disconnect. Cancellation stops
both clients and reaps the modem processes before the station admits another
operation. The fixture stores only receipt and profile digests in history.
[Paired host session](../../rust/src/winlink/host_session.rs) · [Managed daemon](../../rust/src/bin/linkwise-modemd/virtual_host.rs)

The native path still needs tokened playback and PTT events, separate byte
accounting for acknowledged and unresolved data, and qualification of the
admitted live client for disconnect, loss, restart, and timeout. [Modem runtime evidence](../../rust/docs/modem-runtime.md)

## External qualification gates

The review workstation exposed one built-in speaker output and no station
audio input, USB radio, or CAT serial interface. The documented UTM reference
guest and stock VARA/Winlink Express installation were unavailable there.
Virtual waveform and local P2P fixtures were available. [Reference VM setup](../testing/utm-test-environments.md)

1. **Reference modem and client.** Freeze the Rocky server candidate and
   waveform profile hash. Provide access to a Windows reference peer with stock VARA HF 4.9.1,
   a calibrated VB-CABLE/WASAPI route, the command/DATA/audio bridge, and the
   named Winlink Express build. Keep the reference routing isolated and have
   both caller and responder roles available. Save command transcripts,
   both audio directions, exact payload hashes,
   application delivery markers and both disconnects. Close the BW500
   ACK-loss and mixed Pat repeat rows on one build, including the Pat to
   Winlink Express sender-side close. The documented BW2300/BW2750 three-pass
   rows already passed for `70cf5ed` in radio-free guarded recovery and need
   requalification if the shared runtime changes them. [Interoperability matrix](../testing/vara-interoperability.md)
2. **Radio and audio route.** Provide the intended station hardware with the
   exact transceiver, firmware, Hamlib backend, OS audio backend, and calibrated
   input/output interface. Supply an inhibited or conducted test route with
   an independent measurement of actual output and PTT state. Measure
   presentation, turnaround, stop latency and release under
   lost audio, device/host loss, concurrent control and readback mismatch.
   Exercise the physical emergency stop and prove the release deadline holds
   during a process stall or crash. CPAL supplies a presentation estimate
   and the preview leaves physical PTT to external equipment. Implement and
   qualify the single permit arbiter, independent watchdog, commissioning
   profile, and latched recovery before enabling live key-up. [Safety requirements](safety-security-reliability.md) · [Modem qualification criteria](../whitepaper/modem-software-architecture.tex)
3. **One commissioned RMS session.** Provide an approved station profile,
   a permitted band/channel, an established authorized Winlink account and
   client, a compatible gateway, and an agreed test message and test window.
   Record the revision, verified radio state, permit and release evidence and
   confirmed transfer outcomes for one successful exchange and representative
   failure/recovery cases. The Winlink Development Team's onboarding notice
   places new production API and new client CMS access requests on hold.
   Add operating-system and hardware
   profiles only after each passes the same relevant gates. [Winlink developer notice](https://winlink.org/content/developers) · [Use cases and verification](requirements-and-use-cases.md)

## Integration tests

- The service tests use simulated radio and local fixtures.
  They cover deterministic ranking and deduplication, scheduled refresh and
  last-complete fallback, single-use grants and idempotent start, rejection of
  changed readback and expired grants, timeout and cooldown, a release fault
  latch, interrupted-attempt recovery across restart, and rejection of
  inconsistent persisted records or a failed inventory journal. [Station service tests](../../rust/tests/olm_station.rs)
- A process-level smoke test negotiated MCP protocol `2025-06-18` and
  discovered tools over stdio. It exercised inventory refresh, ranking,
  preparation, authorization and start with a shadow profile. Repeating
  start returned the same attempt ID. The completed attempt appeared once
  in history and metrics. Direct shadow CAT writes and key-up were denied,
  simulated key-down was confirmed, and closing stdin ended the service
  cleanly. [MCP service](../../rust/src/mcp.rs)
- The managed BW500 MCP test uses the pinned waveform profile. It aborts a
  connected pair, checks that its private mailbox and processes are cleaned
  up, and starts a fresh attempt. The new attempt verifies both exact
  attachment hashes and records one completed local B2F transfer with CMS
  and recipient status unknown. [Managed waveform test](../../rust/tests/olm_station.rs)
- The virtual modem regression passed for BW500, BW2300 and BW2750, including
  repeated DATA recovery, queued disconnect, and reconnection after losing
  either host socket or the audio socket. The blocked waveform interruption
  and pending-command TCP detachment regressions also passed. The Winlink
  B2F P2P fixture verified attachment hashes and clean close in both
  directions between Linkwise peers. [Modem regression](../../tools/test-virtual-bw500.py) · [Winlink fixture](../../tools/test-winlink-modem.py)
- A separate local `--pat` run on November 25, 2026 used the official Pat
  `v1.0.1` source at `2ad31e8fc1f1748ac1b00cac9d88da54e3e0efe2`, built with
  Go `1.26.11` for macOS ARM64. The BW500 level-4 profile SHA-256 was
  `7fe1eed44ca50f20cb7f64e1e70b9e21d8fe9c348a920609b9888bfdde82bb48`.
  A sandbox check allowed loopback TCP and denied non-loopback TCP and UDP
  before the fixture ran. Pat and Linkwise exchanged B2F mail over virtual
  waveform audio, with matching 1,400-byte attachment hashes in both
  directions and clean disconnect at both modems. [Pat release](https://github.com/la5nta/pat/releases/tag/v1.0.1) · [P2P fixture](../../tools/test-winlink-modem.py)
