# Rust implementation guide

This guide explains the current Rust radio-control implementation for
contributors. It describes what the code does today; proposed modem, session,
and safety-supervisor behavior is not implied by the architecture names.
For an MCP client's call sequence and JSON examples, see
[the MCP usage guide](mcp-usage.md).

## Crate boundaries

The crate is split at the hardware and protocol seams:

```text
MCP stdio transport (`mcp`)
          |
          v
RadioControlPort (`radio` contract)
       /          \
RigctldRadio   SimulatedRadio
       |
private TCP connection to Hamlib rigctld
```

- `mcp` owns MCP tool registration, JSON request/result envelopes, and the
  JSONL audit boundary. It delegates radio operations through the port.
- `radio::RadioControlPort` is the async application boundary. It lets the
  MCP service use real and simulated backends without depending on their
  implementation details.
- `RigctldRadio` translates typed operations to rigctld commands, serializes
  commands with a mutex, applies local write limits, checks readback, and
  maintains a process-local revision counter.
- `SimulatedRadio` stores a small in-memory state. It is not a fake Hamlib
  catalog and does not model hardware timing or external controllers.
- `modem` implements BW500 receive/synthesis primitives and the handshake
  controller; `audio::device` optionally captures native 48 kHz audio through
  CPAL. These are separate from the radio MCP service. See [modem status](modem.md).
- `modem::runtime` serializes establishment/ARQ and simulated device execution;
  see the [runtime guide](modem-runtime.md). It does not open native devices or
  host TCP listeners.
- `api` implements the bounded host command decoder; `platform` remains a planned boundary.

Relevant source: [`lib.rs`](../src/lib.rs), [`mcp.rs`](../src/mcp.rs), and
[`radio.rs`](../src/radio.rs).

## Important contracts

### Radio port

`RadioControlPort` is the contract between MCP and a backend. Implementations
must be `Send + Sync`; its async methods are called concurrently by the server.
The rigctld implementation serializes CAT commands internally. Preserve that
single-command ordering when adding paths that access rigctld. Methods return
`RadioError` for backend, support, policy, protocol, or readback failures; the
MCP layer converts those errors to a stable result status and text.

### State and revisions

`RadioState` is a best-effort snapshot: unavailable fields are `None` and
field-specific failures are collected in `field_errors`. `observed_at_utc`
timestamps the snapshot. `state_revision` is a Linkwise-local write counter,
not a Hamlib or hardware revision; an external controller can change the radio
without incrementing it. `expected_revision` on a write is therefore a local
stale-request guard, not a distributed lock or proof that the physical state
has not changed.

### Capability descriptors and values

`ControlDescriptor.control_id` is the stable identifier used by the MCP get
and set tools. `supported` means the backend advertises the control;
`readable` and `writable` describe the current backend and configured policy.
`value_type` selects the corresponding `ControlValue` variant. `minimum`,
`maximum`, and `enum_values` are advisory constraints when present; the write
implementation must still validate every value independently. A missing limit
does not mean unlimited authority.

`ControlValue` uses a serde tagged representation (`type` plus `value`) for
MCP JSON. Generic Hamlib levels and parameters currently use numeric values,
functions use booleans, and text-valued controls use strings. Parameter enum
typing is not yet implemented. Do not silently coerce one variant into another:
reject a mismatched control/value pair as unsupported or denied.

### CAT write path

The rigctld write implementation checks global write enablement, per-control
profile enablement for extended controls, expected local revision, and PTT
state. Frequency, mode, and power have additional configured limits. It then
sends a CAT command and reads the requested control back. Return success only
after the implementation has verified the write to the extent the backend
supports. Keep writes disabled by default. A new control must not bypass these
checks just because it has a new command mapping.

### PTT and audit behavior

`radio_set_control` appends a request record before attempting the write; if
that append fails, the write is denied. It appends a completion record after
the operation. If completion logging fails, the operation may already have
happened, so callers receive `audit_failure` and must treat the outcome as
uncertain.

`radio_ptt_request` is deliberately asymmetric: key-up fails closed when the
initial audit append fails, while key-down is still allowed so release can
proceed. The current rigctld backend denies key-up because no session/permit
arbiter is implemented. `radio_get_ptt_state` reports no active watchdog.
Do not describe the current code as transmit-qualified or infer authorization
from `session_id` or `transmit_permit_id`; those fields are not currently
validated by the backend.

## Error/result mapping

| Radio error | MCP `status` | Interpretation |
| --- | --- | --- |
| `PolicyDenied` | `denied` | Local configuration or state refused the operation. |
| `Unsupported` | `unsupported` | The backend/control/value combination is unavailable. |
| `Unavailable` | `unavailable` | Connection or timeout failure. |
| `ReadbackMismatch` | `readback_mismatch` | Requested and observed values differ, or required readback failed. |
| `Protocol` | `failed` | rigctld response or parsing failed. |

The MCP layer can also return `audit_failure`. For writes, clients should
refresh state/control observations before retrying after an uncertain result.

## Adding or changing a radio control

Follow the complete path so the typed API and runtime behavior remain aligned:

1. Add or select a `ControlValue` variant that preserves the control's units
   and value shape. Document units and any normalization.
2. Add the control descriptor in `RigctldRadio::capabilities`, using backend
   capability data and configured policy to set support/read/write fields.
3. Implement read mapping in `control` and write mapping in `set_control`.
   Validate IDs, enum values, numeric ranges, side effects, write enablement,
   PTT state, and readback. Never build a rigctld command from unchecked
   caller text.
4. Decide how the simulator behaves. Either model the operation explicitly
   with tests or report it as unsupported; do not accidentally imply the
   simulator covers hardware behavior.
5. Update MCP schema/description where the caller-visible contract changes,
   then update [`mcp-usage.md`](mcp-usage.md) examples and this guide if an
   implementation invariant changes.
6. Add tests for parsing/serialization, allowed and denied values, stale
   revisions, backend errors, and readback behavior as appropriate.

For Hamlib catalog additions, prefer parsing backend capability data to
hard-coded assumptions. Keep vendor extensions and operations without a safe
typed mapping explicitly unsupported until their schema and side effects are
understood.

## Build, run, and inspect

Use the repository's Compose profiles for service runs. From the repository
root:

```sh
docker compose --profile dev run --build --rm -i linkwise
```

The `dev` profile uses the simulator by default. To exercise the Hamlib-backed
path, use the `radio` profile and configure its `hamlib` service; see the
repository README and `.env.compose.example` for the full container settings.
Do not connect a physical radio or enable CAT writes as part of an ordinary
build check.

For Rust-only checks from `rust/`:

```sh
cargo fmt --check
cargo check --locked
cargo test --locked
cargo doc --no-deps --locked
```

Generated API documentation is under `target/doc/linkwise/`. Keep Rustdoc
examples focused on stable contracts; backend-specific control availability
belongs in runtime capability responses and the MCP usage guide.
