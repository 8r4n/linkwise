# Linkwise MCP interface design

**Document ID:** LINKWISE-ICD-MCP-001  
**Version:** 0.1, proposed  
**Status:** Interface proposal; partial Rust server implemented. See the
implementation progress section of the architecture whitepaper for the
implemented subset and validation limits.

## 1. Design goals

- Expose useful radio/link state and bounded operations to MCP-capable AI hosts.
- Keep tool schemas typed, narrow, and purpose-based.
- Make observation non-mutating and actuator calls policy-mediated.
- Return machine-readable evidence, state, freshness, and error reasons.
- Enable the AI host to use the complete CAT control set advertised by the
  radio backend and explicit PTT controls through Linkwise tools, without a
  bypass around the adapter, station policy, audit trail, or safety supervisor.
- Represent ordinary controls as typed capability/value operations. Support
  radio-specific CAT extensions through registered schemas and side-effect
  classifications rather than blind command strings. Expose PTT status,
  authorized key-up, and unconditional key-down separately.

MCP standardizes discovery and invocation of server tools. It does not itself
authorize the physical actions performed by an integration; authorization,
policy, and action safety are Linkwise responsibilities [1].

## 2. Transport and trust

Initial deployment should use local stdio transport when launched by an AI
host, or loopback-only Streamable HTTP if a host requires a network transport.
Do not bind an unauthenticated MCP endpoint or rigctld to a public or shared
network. Remote transport requires authenticated identity, authorization,
transport security, replay protection, and explicit deployment review.

The MCP process maps model requests to an internal typed command API.
Adapters are not directly reachable by the model. Every mutation carries the
autonomous service identity, correlation ID, expiry, and policy evaluation.
The station profile is installed before autonomous service start; runtime
human-facing controls are telemetry plus an independent emergency stop, not
approval or routine-operation tools.

## 3. Resources

| Resource URI | Access | Contents |
| --- | --- | --- |
| `linkwise://station/profile` | Read | Configured station capabilities and operating limits; secrets omitted |
| `linkwise://radio/state` | Read | Last readback snapshot, per-field freshness/capability/error |
| `linkwise://modem/state` | Read | Modem lifecycle, signal/session status, negotiated parameters |
| `linkwise://winlink/channels` | Read | Normalized channel snapshot with source timestamp and expiry |
| `linkwise://winlink/inventory` | Read | Counts by unique gateway, channel, eligibility, protocol, band, service, and report-age bucket; source/snapshot timestamps |
| `linkwise://winlink/queue-summary` | Read/optional | Aggregate queue counts/bytes only when supported by the selected client; no message body/recipient |
| `linkwise://links/candidates` | Read | Current eligible candidates and evidence-backed ranking |
| `linkwise://links/metrics` | Read | Attempt, connection, and transfer counts/rates with window, denominator, and data coverage |
| `linkwise://propagation/status` | Read | Provider health, latest observation/forecast times, freshness, coverage, and adapter errors |
| `linkwise://propagation/summary/{path_id}` | Read | Latest path analysis with evidence IDs, model/ranker versions, assumptions, uncertainty, and stale/missing inputs |
| `linkwise://links/analysis/{candidate_id}` | Read | Immutable analysis snapshot and score explanation for one candidate |
| `linkwise://sessions/{session_id}` | Read | Session lifecycle and summary, never message body by default |
| `linkwise://audit/{correlation_id}` | Read/restricted | Decision and action trace under local operator authorization |

Resource content is informational. A tool must revalidate state at execution
time and may not rely on a previously read resource snapshot.

## 4. Tools

### 4.1 Read-only tools

| Tool | Inputs | Output summary |
| --- | --- | --- |
| `radio_get_state` | Optional `max_age_ms` | Current state, per-field support/freshness, radio identity |
| `radio_get_capabilities` | None | Supported fields/ranges/enums from backend and local profile |
| `radio_get_controls` | Optional control/category filters | Backend-advertised control catalog, current values, ranges/enums, access classes, and policy availability |
| `radio_get_control` | `control_id` | Current value and support, freshness, and readback status |
| `radio_get_ptt_state` | None | Configured PTT route, requested/observed state, readback support, and active watchdog status |
| `modem_get_state` | None | Modem status and client connection state |
| `winlink_refresh_inventory` | Optional `max_age_ms`, protocol/service filters | Immutable normalized snapshot ID, count delta, source/retrieval times, warnings |
| `winlink_get_inventory_summary` | Optional protocol/band/service filters | Labeled unique-gateway/channel/eligibility/status-age counts, `as_of`, snapshot ID, warnings |
| `winlink_get_gateway_channels` | Gateway identity | All channels for one station with protocol/frequency/source/freshness |
| `winlink_get_queue_summary` | Optional direction/time window | Aggregate pending queue counts/bytes; capability error if unsupported |
| `link_rank_gateways` | Objective, allowed band/modes, optional candidate IDs | Immutable ranking snapshot ID; ranked candidate links and ranking-entry IDs, score components, analysis/input provenance, freshness, uncertainty, and exclusions |
| `link_get_history` | Time range, gateway/channel, result filters | Distinct session and transfer events; message content excluded |
| `link_get_metrics` | Time window and optional filters | Attempted/connected/failed sessions and client-confirmed transfer outcomes with numerator, denominator, and coverage |
| `propagation_get_status` | Optional provider IDs | Provider health, freshness, coverage, and latest successful collection time |
| `propagation_get_path_analysis` | Station/path, channel/frequency, UTC planning window, optional `max_age_ms` | Versioned analysis ID and exact input IDs/roles, model/config and station-profile revisions, features, uncertainty, coverage, and missing/stale inputs |
| `link_get_candidate_analysis` | Candidate ID or ranking-entry ID | Exact immutable analysis and ranking snapshot IDs; candidate/ranking-entry IDs, associated decision if any, exclusions, policy result, and full provenance |
| `link_get_similar_sessions` | Path/gateway/frequency filters, UTC time window, optional similarity dimensions | Historical aggregate ID/version, matching dimensions and window, measured outcome counts/denominators, sample size, coverage, and uncertainty; source attempts/events remain addressable |
| `propagation_refresh_sources` | Configured provider IDs, bounded deadline | Collection-run IDs, per-provider success/partial/failure status, timing, coverage/warnings, and immutable evidence IDs; never changes radio state |

`winlink_refresh_inventory` is read-only with respect to radio state but may
perform network I/O. Rate limits, timeout, source attribution, and cache age
must be explicit. It returns an immutable snapshot ID and count delta;
`winlink_get_inventory_summary` reads that normalized snapshot. Refreshing a
directory is not a live RF test and does not verify a gateway is reachable.
`winlink_get_queue_summary` is optional and must return unsupported if the
chosen client exposes no supported queue interface; no GUI scraping fallback.

Propagation collection and analysis are scheduled application services, not
work performed by MCP tool handlers. `propagation_refresh_sources` only
requests a bounded refresh of preconfigured providers; the caller cannot
provide arbitrary URLs, credentials, polling intervals, or parser rules. A
tool response includes immutable collection-run, evidence, analysis, ranking,
and aggregate IDs so later queries can recover the exact inputs and outcomes
used. Every analysis input identifies one immutable target and its role.
Historical summaries are computed from persisted, deduplicated measured
events by Linkwise, not reconstructed from the AI conversation. Each summary
identifies its version, time window, match dimensions, counts and denominators,
coverage, freshness, and uncertainty; sparse history remains explicitly
sparse. A partial or failed refresh preserves the previous valid data as
stale, reports per-provider status and warnings, and never substitutes zeros.
Decision responses distinguish select, wait, and no-route; retries are
represented as separate session attempts linked back to the originating
decision, with outcome stages kept distinct.

### 4.2 Capability-driven CAT control tools

The MCP surface is not limited to the subset used by automatic gateway
selection. `radio_get_controls` publishes every control the selected Hamlib
backend can read or set, including its supported VFO, frequency, mode/filter,
power, antenna, split, levels, functions, memories, and other controls. The
actual set is radio/backend-specific; unsupported operations are reported,
not fabricated. `radio_set_control` accepts a `control_id` and a
schema-validated value. It can set any advertised control, subject to a fresh
state read, exclusive lease, authority mode, station profile, and local policy.
Changes are serialized and read back; non-reversible or disruptive controls
are explicitly classified and may require a more restrictive autonomous
policy class or be disabled for autonomous execution.

For radio features not represented by common Hamlib controls, the adapter may
register `radio_invoke_extension(extension_id, typed_arguments)`. Each
extension declares its backend/version scope, schema, side effects, and access
class (read-only, operational, disruptive, or service-level). Unknown CAT
strings/bytes, shell commands, and direct rigctld sockets are not MCP inputs.
The registry makes device-specific capability available without letting
model-generated protocol syntax bypass policy or audit. A local operator can
enable qualified extensions for a particular radio profile.

| Tool | Inputs | Effect |
| --- | --- | --- |
| `radio_set_control` | `control_id`, schema-validated `value`, optional expected state revision | Apply any supported/locally permitted CAT control under lease; verify and return readback |
| `radio_invoke_extension` | Registered `extension_id`, typed arguments, expected state revision | Invoke a catalogued device-specific CAT operation under its declared policy/approval class |

### 4.3 Preparation tools

Preparation writes radio configuration but cannot assert PTT or start a modem
session.

```json
{
  "gateway_id": "W1XYZ-10/28120000/VARA-HF",
  "requested_power_w": 50,
  "objective": "deliver_queued_message"
}
```

Tool: `link_prepare_candidate`. It re-fetches/validates the candidate, verifies
policy, takes the exclusive lease, sets frequency/mode/bandwidth/power through
Hamlib, performs readback, and returns a short-lived `prepared_handle` bound to
that exact state. It must leave PTT deasserted.

The client must not be allowed to supply arbitrary frequency beyond the local
station profile. The handle becomes invalid on any external radio state
change, policy revision change, timeout, lease loss, or gateway snapshot
invalidation.

### 4.4 Session tools

`link_request_session_authorization` asks local autonomous policy for a grant
bound to gateway, frequency, modem, client, message count or queue scope,
maximum power, and maximum session duration. If every preconfigured rule and
current interlock passes, policy issues the grant automatically; otherwise
the request is denied and the autonomous planner selects a safe wait, retry,
or alternative. No runtime human approval is requested. The LLM itself cannot
mint or widen the grant.

`link_start_session` requires `prepared_handle` and a one-use authorization
token. It starts the supported Winlink client session and returns
`session_id`. For normal VARA/Winlink operation, the modem/client requests
keying through the PTT arbiter; the AI does not manually sequence modem
keying.

### 4.5 PTT control tools

PTT is a first-class actuator with separate state reporting, key-up, and
release semantics. The AI host can actively control PTT through
`radio_ptt_request`; it is not restricted to modem-originated requests.
`radio_get_ptt_state` reports route, requested state, observed state, and
readback capability. `radio_ptt_request` accepts `action` (`key_up` or
`key_down`), an optional `session_id`, and—only for `key_up`—a one-use
`transmit_permit_id`. A key-up request is accepted only when the named session
is active, owns the exclusive radio lease, has a current policy authorization,
matches the verified radio configuration, and is within locally configured
per-key-up and total-session limits. The caller cannot choose or raise those
limits. The PTT arbiter then uses the configured supported control route
(radio backend/CAT, serial control line, or another qualified adapter) and
confirms state when the hardware supports readback.

`link_request_ptt_permit` is available only during a session already covered
by an unexpired session authorization. It asks the local policy engine to
revalidate state and issue a single-use, short-lived permit for one bounded
key-up. The model cannot mint permits, change the limits, or request a permit
for a different channel/configuration. Permit issuance is audited and consumes
the session's configured burst/duty budget.

Example key-up request (the caller cannot supply a duration or override any
station limit):

```json
{
  "action": "key_up",
  "session_id": "session-uuid",
  "transmit_permit_id": "permit-uuid"
}
```

The response reports the accepted/denied decision, observed key state, and
policy-selected maximum key-up interval. At that interval the arbiter
deasserts PTT automatically; another key-up requires revalidation. Exceeding
a session duty/time budget aborts the session rather than extending keying.

`key_down` is idempotent, does not require a session ID or transmit permit, and is accepted
from the session controller, safety monitor, or AI-facing tool
surface. An AI request to key down may only make the station safer. There is no
operator software key-down/abort control during runtime; the separate physical
emergency stop is the human actuator.
`link_abort_session` also forces release. The server records actor, session, permit, requested
action, observed result, and timestamps. No arbitrary-duration key-up,
unscoped `set_ptt(true)`, or direct Hamlib command is exposed. If the modem
adapter owns PTT internally, it uses the same arbiter and watchdog rather than
bypassing policy. In normal VARA operation, Linkwise owns end-to-end PTT
coordination but the modem remains responsible for protocol-accurate burst
timing; AI-facing PTT tools provide explicit bounded control, not hard-real-time
waveform keying.

`link_get_session` returns status, elapsed time, modem metrics, transfer
progress, and termination reason. `link_abort_session` is idempotent and
available to the autonomous session controller and safety policy. `link_abort_session` does
not require a model-provided reason to take effect.

## 5. Common response envelope

All tool results should include:

```json
{
  "request_id": "uuid",
  "correlation_id": "uuid",
  "observed_at_utc": "RFC3339 timestamp",
  "status": "ok | partial | denied | stale | failed",
  "data": {},
  "warnings": [],
  "errors": []
}
```

State values include source timestamps. Missing, unsupported, invalid, and
unknown fields are distinct. Never serialize an unavailable numeric measurement
as zero.

## 6. Error taxonomy

| Code | Meaning | Expected handling |
| --- | --- | --- |
| `RADIO_UNAVAILABLE` | rigctld/backend not responding | No state changes; retry per policy |
| `CAPABILITY_UNSUPPORTED` | Requested operation not supported | Reject; do not emulate silently |
| `CHANNEL_STALE` | Candidate inventory expired | Refresh inventory, re-rank |
| `POLICY_DENIED` | Local constraints disallow action | Return rule ID; autonomous planner waits, recovers, or selects an eligible alternative |
| `LEASE_BUSY` | Another controller owns radio | Do not race; wait with bounded backoff and retry |
| `READBACK_MISMATCH` | Radio did not accept requested config | Invalidate prepared handle; inhibit session |
| `CHANNEL_BUSY` | Occupancy check disallows attempt | Wait/re-evaluate; no PTT |
| `AUTH_REQUIRED` / `AUTH_EXPIRED` | Session grant missing/expired | Re-evaluate installed autonomous policy and request a fresh bounded grant |
| `SESSION_TIMEOUT` | Duration or watchdog budget exceeded | Abort, PTT release, safe state |
| `PTT_DENIED` | Key-up lacks valid session/permit, lease, verified configuration, or remaining budget | Keep/deassert PTT; return the failed policy condition |
| `PTT_RELEASE_UNCONFIRMED` | Release was requested but hardware state cannot be confirmed | Abort; latch transmit inhibit; remain faulted until safe maintenance |
| `ADAPTER_ERROR` | Modem/client/source failed | Return partial state, preserve audit trace |

## 7. Tool annotations and permission classes

Every tool is labeled internally with one permission class:

1. `READ`: no external side effect except bounded retrieval.
2. `RADIO_CONTROL`: applies an advertised, enabled CAT operation under the
   installed policy and exclusive lease; no human approval is requested.
3. `PREPARE`: changes radio configuration, no RF transmit.
4. `SESSION_AUTO`: may lead to RF transmission; deterministic local policy
   must issue a bounded grant and all interlocks must pass.
5. `ABORT`: autonomous safety action; always permitted, idempotent, high
   priority.
6. `PTT_KEY_UP`: requires an active policy-authorized session and one-use permit;
   duration and duty cycle are locally capped.
7. `PTT_KEY_DOWN`: always permitted and idempotent; release only.
8. `EMERGENCY_STOP`: human-only physical control, outside MCP and independent
   of application software/network availability.

MCP annotations such as read-only/destructive hints are useful to clients but
are not enforcement. The server-side policy engine remains authoritative.

## 8. Versioning

Tool names and JSON fields are versioned as part of the Linkwise API. Additive
optional fields are preferred. Breaking behavior requires a new API version
and explicit migration. Returned policy version and adapter versions are
recorded for each mutating action.

## 9. References

1. Model Context Protocol, “Server Tools,”
   <https://modelcontextprotocol.io/specification/2025-06-18/server/tools>.
2. Hamlib, “Network Device Control,”
   <https://github.com/Hamlib/Hamlib/wiki/Network-Device-Control>.
