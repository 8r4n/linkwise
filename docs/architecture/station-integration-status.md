# Station integration status

The station service can import and rank RMS channel records. In shadow mode it
can prepare a simulated radio, enforce short-lived preparation and authorization
handles, and record a bounded attempt. Those attempts use scripted outcomes.
The two-way modem preview independently exchanges actual waveforms over virtual
or native audio. Connecting the two paths through one session owner is the next
integration milestone. The live autonomous station release remains gated on
hardware transmit control and a qualified Winlink gateway exchange.

## Implementation and evidence

| Requirements | Implemented and evidenced | Completion gate |
| --- | --- | --- |
| OVR-F-003–006, 021, 023, 034–036 | The station imports bounded, versioned inventory snapshots and optional VOACAP results from configured files. It records hashes and source times, distinguishes gateway and channel counts, and explains exclusions and the ranker's select, wait, or no-route result. The scheduler refreshes the inventory file independently of MCP calls. | Implement and qualify the authorized live RMS source adapter and a pinned VOACAP runner. Record per-path assumptions, model/run IDs, collection health and freshness. Add local observations, antenna fit and measured outcome history to the scoring model. [Station service](../../rust/src/olm.rs) · [Inventory and forecast inputs](../../rust/src/olm/inventory.rs) |
| OVR-F-002, 007–014, 018–020, 022, 033 | Shadow mode owns the simulated radio configuration, verifies USB/zero power/PTT-down readback, and journals preparation, grants and attempt outcomes. It exposes bounded result history through MCP and preserves separate modem, client, CMS and recipient outcome fields. The Hamlib path verifies typed writes and supports a separate release request. Key-up is denied by the current radio adapters. | Introduce an admitted modem/client operation bound to the station lease, expected client and peer, verified configuration revision, and expiry. Record actual client/session events by ID. Rerun the shadow lifecycle cases with the integrated bearer and test losing modem/client ownership during active I/O. [Station service](../../rust/src/olm.rs) · [Radio adapter](../../rust/src/radio.rs) · [MCP tools](../../rust/src/mcp.rs) |
| OVR-F-013, 020, 031 and OVR-Q-001, 009 | The paired command and DATA sockets detach on EOF, including while a backend reply is pending. `CONNECT` and immediate `DISCONNECT` acknowledge admission before waveform I/O. Decoded `ABORT` takes priority over pending waveform work. Forced service stop cancels local playback without sending a disconnect burst. An interrupted virtual record requires a new cable/host attachment. If native presentation is unconfirmed, the service closes the host listeners and waits for restart. | Use a single session owner for the native and virtual modem paths, with tokened device events, explicit cancellation and bounded byte accounting. Qualify buffering, channel loss, wrong owner, responder admission and abort under simultaneous activity. [TCP host](../../rust/src/host/tcp.rs) · [Waveform engine](../../rust/src/bin/linkwise-modemd/virtual_host.rs) · [Native audio](../../rust/src/bin/linkwise-modemd/station.rs) |
| OVR-F-025–030, 037–040 | Shadow scenarios exercise the station tool boundary with scripted timing and outcomes. Historic rankings remain queryable by ID after restart. The separate adaptive network bench provides seeded contact simulations and durable independent-station workflows. | Run the station tool contracts with a virtual-time RF and outcome model. Compare policies on paired held-out scenarios and publish denominators and uncertainty. Recompute historic rankings from their recorded inputs and validate the results independently. [Simulation design](rf-agent-simulation-design.md) · [Network bench](../../rust/network/README.md) |

## First integrated software milestone

1. Add a bounded internal session interface for a radio-free BW500 fixture.
   The fixture owns both waveform endpoints and prevents access to native
   devices. The station's accepted start yields one operation identity. A
   matching client attachment consumes the reservation once, and a mismatch
   or expiry produces a terminal result. The legacy TCP connection alone
   cannot identify its owning client or prove that audio stays virtual.
2. Transfer arbitrary binary data through both roles. Preserve exact payload
   hashes and account for pending, acknowledged, unresolved and discarded
   bytes. Exercise lost acknowledgments, duplicate DATA, channel loss,
   cancellation, client reconnect, process interruption and backend outages.
   Distinguish local device cancellation from observed peer disconnect. The
   existing waveform engine owns measured wire sequencing. Preserve those
   behaviors as they move behind the shared portable session contract.
3. Add one named Winlink client adapter and capture which stage accepted
   each message. Keep P2P delivery, modem connection, RMS/CMS acceptance
   and recipient confirmation as distinct facts. [Modem runtime evidence](../../rust/docs/modem-runtime.md)

## External qualification gates

1. **Reference modem and client.** Freeze the candidate build and waveform
   profile hash. Provide a working stock VARA HF 4.9.1 installation, a
   calibrated virtual audio route and a named Winlink client/version. Save
   command transcripts, both audio directions, exact payload hashes,
   application delivery markers and both disconnects. Close the BW500
   ACK-loss and mixed Pat repeat rows on one build, including the Pat to
   Winlink Express sender-side close. The documented BW2300/BW2750 three-pass
   rows already passed for `70cf5ed` in radio-free guarded recovery and need
   requalification if the shared runtime changes them. [Interoperability matrix](../testing/vara-interoperability.md)
2. **Radio and audio route.** Select the exact transceiver, Hamlib backend,
   firmware, OS audio backend and input/output interface. Use an inhibited or
   conducted bench with an independent indication of actual output and PTT
   state. Measure presentation, turnaround, stop latency and release under
   lost audio, device/host loss, concurrent control and readback mismatch.
   Exercise the physical emergency stop and prove the release deadline holds
   during a process stall or crash. CPAL supplies a presentation estimate
   and the preview leaves physical PTT to external equipment. Implement and
   qualify the single permit arbiter, independent watchdog, commissioning
   profile, and latched recovery before enabling live key-up. [Safety requirements](safety-security-reliability.md) · [Modem qualification criteria](../whitepaper/modem-software-architecture.tex)
3. **One commissioned RMS session.** Provide an approved station profile,
   a permitted band/channel and a compatible, authorized gateway/client setup.
   Record the revision, verified radio state, permit and release evidence and
   confirmed transfer outcomes for one successful exchange and representative
   failure/recovery cases. Add operating-system and hardware profiles only
   after each passes the same relevant gates. [Use cases and verification](requirements-and-use-cases.md)

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
- The virtual modem regression passed for BW500, BW2300 and BW2750, including
  repeated DATA recovery, queued disconnect, and reconnection after losing
  either host socket or the audio socket. The blocked waveform interruption
  and pending-command TCP detachment regressions also passed. The Winlink
  B2F P2P fixture verified attachment hashes and clean close in both
  directions between Linkwise peers. [Modem regression](../../tools/test-virtual-bw500.py) · [Winlink fixture](../../tools/test-winlink-modem.py)
