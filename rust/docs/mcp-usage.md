# Using the Linkwise radio MCP server

This guide is for an MCP client or AI system that needs to observe or request
radio-control changes. The server currently implements radio observation and a
limited CAT control surface. It does not implement the modem, a transmit
session/permit arbiter, or an active PTT watchdog. Treat runtime tool schemas
and `radio_get_controls` as authoritative for a particular backend; examples
below illustrate the value shapes.

## Recommended call sequence

1. Call `radio_get_state` to observe the current radio state and its
   `state_revision`.
2. Call `radio_get_controls` to discover controls for the selected backend.
   Choose a control only when `supported` is true. For reads, require
   `readable`; for writes, require `writable`.
3. Use the exact `control_id` returned by the server. Do not invent IDs or
   infer support from another radio model.
4. For a read, call `radio_get_control` with that ID. For a write, match the
   descriptor's `value_type` to the tagged `ControlValue` variant and respect
   its range and enum values. Pass the latest `state_revision` as
   `expected_state_revision` when available.
5. Inspect the complete result. A write is successful only when `status` is
   `ok`; use its returned state/readback and warnings to determine what was
   observed. A request being accepted is not proof of a later radio state or
   successful message transmission.

Example read-only discovery:

```json
{}
```

Call `radio_get_state`, then `radio_get_controls`. A control descriptor is
similar to:

```json
{
  "control_id": "frequency_hz",
  "supported": true,
  "readable": true,
  "writable": false,
  "value_type": "frequency_hz",
  "minimum": 7100000,
  "maximum": 7200000,
  "enum_values": [],
  "access_class": "read_only",
  "side_effect_class": "operational"
}
```

Fields and limits vary by backend and station profile. A descriptor with
`writable: false` must be treated as read-only; do not repeatedly retry a
denied write.

## Tool reference

| Tool | Purpose | Inputs / important behavior |
| --- | --- | --- |
| `radio_get_state` | Observe common radio state. | No arguments. Returns per-field errors, timestamp, and Linkwise-local `state_revision`. |
| `radio_get_controls` | Get the concise discovered control catalog. | No arguments. Use descriptor flags, ranges, enums, and exact IDs to select controls. |
| `radio_get_capabilities` | Get catalog plus raw backend capability dump. | No arguments. Use for diagnostics; the raw dump may be large. |
| `radio_get_control` | Read one control. | `{ "control_id": "<exact ID from catalog>" }`. Returns a tagged `ControlValue`. |
| `radio_set_control` | Request a policy-checked write. | `{ "control_id": "<exact writable ID>", "value": <tagged ControlValue>, "expected_state_revision": <latest revision or null> }`. Physical writes need server-side configuration and can still be denied. |
| `radio_get_ptt_state` | Observe PTT and route/watchdog metadata. | No arguments. `watchdog_active` is currently false. |
| `radio_ptt_request` | Request PTT key-up or key-down. | Key-up is currently denied because the session/permit arbiter is not implemented. Key-down requests release. This prototype is not qualified for transmission. |

Every tool returns an `ApiResult` envelope with `status`, optional `data`,
`warnings`, `errors`, timestamps, and request/correlation IDs. For writes,
`status: "ok"` means the adapter completed its operation and returned
readback; it does not mean an end-to-end radio session or message succeeded.

PTT request shape, for clarity:

```json
{
  "action": "key_down",
  "session_id": null,
  "transmit_permit_id": null
}
```

`action` is `key_up` or `key_down`. A key-up request is denied by this
implementation regardless of supplied IDs; the optional fields are part of
the request schema and are not authorization credentials by themselves.

Common non-`ok` statuses include:

| Status | Meaning | Client response |
| --- | --- | --- |
| `denied` | Local policy rejected the request, such as disabled writes, disallowed value, PTT asserted, or stale revision. | Do not retry unchanged. Refresh state/catalog or explain that policy blocks the request. |
| `unsupported` | The backend/control does not expose the requested operation or value. | Choose a listed supported control; do not emulate it. |
| `unavailable` | The backend could not be reached or responded before timeout. | Report the interruption; retry only after checking current state and avoiding duplicate actuation. |
| `readback_mismatch` | Observed value did not match the requested value. | Treat radio state as uncertain; read state and the specific control before any further action. |
| `audit_failure` | An action could not be recorded or its completion could not be logged. | Treat outcome as uncertain. Read state before any retry. |
| `failed` | Protocol or other adapter error. | Include the returned error and correlation ID; refresh state before retrying a write. |

## Tagged control values

`radio_set_control.value` uses a tagged union. The `type` identifies the
variant and `value` contains its payload. Use only a variant compatible with
the selected descriptor:

```json
{ "type": "frequency_hz", "value": 7100000 }
{ "type": "mode", "value": "USB" }
{ "type": "rf_power_fraction", "value": 0.2 }
{ "type": "number", "value": 0.5 }
{ "type": "boolean", "value": true }
{ "type": "text", "value": "VFOA" }
{ "type": "passband_hz", "value": 2400 }
{ "type": "integer", "value": 3 }
{ "type": "split_vfo", "value": { "enabled": true, "tx_vfo": "VFOB" } }
{ "type": "antenna_index", "value": 1 }
{ "type": "signed_hertz", "value": -250 }
{ "type": "mode_passband", "value": { "mode": "USB", "passband_hz": 2400 } }
```

Examples of ID-to-type mappings include `frequency_hz` → `frequency_hz`,
`mode` → `mode`, `level:RFPOWER` → `number`, `function:NB` → `boolean`,
`vfo` → `text`, `split_vfo` → `split_vfo`, `rit_hz`/`xit_hz` →
`signed_hertz`, and `split_mode` → `mode_passband`. The descriptor's
`value_type`, `minimum`, `maximum`, and `enum_values` take precedence over
these examples. `rf_power_fraction` is Linkwise's normalized 0.0–1.0 form;
generic Hamlib level values use the backend's reported scale.

Example write request shape (only valid if the live descriptor says the
control is writable and the configured policy permits the value):

```json
{
  "control_id": "frequency_hz",
  "value": { "type": "frequency_hz", "value": 7100000 },
  "expected_state_revision": 4
}
```

## Policy and limits

- The default backend is the in-memory simulator. It supports only its
  documented frequency, mode, and RF-power operations; it has no hardware
  route and always denies key-up.
- Physical CAT writes are disabled unless `LINKWISE_RADIO_WRITES=true` and
  station-profile values are configured. Frequency, mode, and RF-power
  writes require their applicable limits. Additional Hamlib controls also
  require their exact ID in `LINKWISE_ENABLED_HAMLIB_CONTROLS`.
- The adapter denies CAT writes while PTT is asserted, checks the optional
  local state revision, and verifies write readback. A revision is maintained
  by this Linkwise process and may not detect a change made by another
  controller.
- `radio_ptt_request` key-up is denied in this implementation. Its request
  fields do not constitute authorization. `radio_get_ptt_state` currently
  reports `watchdog_active: false`; do not use this prototype to transmit.
- Tool descriptions, annotations, or an AI prompt do not enforce safety.
  Server policy is authoritative, and non-`ok` results must be respected.

## Implementation scope

The catalog is backend-discovered and includes common controls plus generic
Hamlib levels, functions, and parameters. Memory contents/records, vendor
extension invocation, typed parameter enums, some non-control rig operations,
external-controller revision tracking, and transmit authorization/watchdog
remain incomplete. Modem and Winlink operations are not implemented by this
MCP server.
