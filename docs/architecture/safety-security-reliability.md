# Linkwise safety, security, and reliability design

**Document ID:** LINKWISE-SSR-001  
**Version:** 0.1, proposed  
**Status:** Design proposal; thresholds unallocated pending station profile

## 1. Safety objective

Linkwise must not permit an AI model, stale data source, client fault, or
software race to create an unbounded or unauthorized RF transmission. The
system autonomously optimizes communication opportunities only inside an
explicitly installed station authority. During runtime, the human has only an
independent emergency stop; commissioning and profile installation happen
before autonomous activation. This document is a software safety design basis,
not a claim of regulatory certification or a replacement for the operator's
license, station procedures, or equipment protections.

## 2. Safety invariants

1. PTT key-up is possible only for an authorized, active session that owns the
   radio lease and whose configuration has been verified. PTT key-down is
   always permitted as a safety action.
2. Only one controller can mutate the radio at a time.
3. Frequency, mode, power, and session duration are validated against a local
   station profile before preparation and again before session start.
4. The AI may use the complete locally enabled CAT control set and explicit
   PTT tools through MCP. It cannot alter policy, raise limits, approve its
   own request, or bypass the radio adapter/command coordinator with raw
   CAT/serial/socket/shell access. Every key-up goes through the PTT arbiter
   and a one-use session permit.
5. No tuning or gateway switch occurs during PTT or active payload transfer,
   except to abort into a safer state.
6. Authorization is one-use, short-lived, bound to a candidate and config, and
   revoked on any material state change.
7. Loss of controller, lease, heartbeat, or watchdog causes client abort and
   PTT release through the available control chain; physical emergency stop is
   independent.
8. Stale or unavailable observations are reported as such, never coerced into
   favorable values.
9. Reentry association and propagation predictions never constitute an RF
   authorization decision.
10. A human-only emergency stop always preempts all operations. No other
    human actuator control or per-session approval is exposed at runtime.

## 3. Hazard analysis (initial)

Severity/likelihood are not assigned until hazard analysis with station
hardware. The rows are candidate hazards and control concepts.

| ID | Hazard | Causes | Preventive/detective controls | Safe response |
| --- | --- | --- | --- | --- |
| H-01 | Unintended transmission | Prompt injection, tool misuse, client starts unexpectedly | PTT arbiter; key-up requires active session, lease, one-use permit and local budget; PTT-state monitoring | Abort, force key-down, latch inhibit, record fault |
| H-02 | Transmission on wrong frequency/mode | Race, external knob change, stale channel record | Fresh channel check; lease; pre/post tune readback; session-start readback | Invalidate handle and prohibit session |
| H-03 | Excessive power or duty cycle | AI requests max power; continuous digital transmit | Local power ceiling; session duration budget; radio protection telemetry where available | Abort, release PTT, latch inhibit if state is uncertain |
| H-04 | Retune during active transfer | Optimizer reacts to score update | State machine forbids retune while busy; candidate changes deferred | Preserve current session unless safety abort |
| H-05 | PTT stuck asserted | Process crash, audio device/driver failure | Independent watchdog, maximum continuous key-up and session time, modem/client routed through arbiter, physical inhibit | Attempt key-down via independent route; physical emergency stop remains available; latch safe pending offline maintenance if unconfirmed |
| H-06 | Bad/stale gateway advice | RMS report delay, wrong mode/band, cached data | Data TTL; capability/mode validation; source timestamp; final refresh | Re-rank or deny start |
| H-07 | Harmful control request via remote MCP | Unauthenticated network binding or stolen client | Local-only default; authentication/authorization for remote; firewall; least privilege | Disable endpoint and revoke grants |
| H-08 | Sensitive message/log exposure | Payload copied into prompt or persisted | Payload-free default; redaction; local retention controls; no full text in audit | Restrict storage, purge per policy |
| H-09 | Repeated link churn | Noisy score inputs or model indecision | Hysteresis, dwell, cooldown, retry budget, switch penalty | Hold stable channel; autonomous policy continues to observe and rank |
| H-10 | AI unavailable during operation | Provider/network failure | Local deterministic watchdog and deterministic fallback policy, independent of model | Complete only bounded safe work or abort to receive-only; no manual runtime mode |

## 4. State machine

```text
SAFE / RECEIVE_ONLY
       │ validated candidate + lease
       ▼
PREPARING ── failure/readback mismatch ──► SAFE / RECEIVE_ONLY
       │ config verified; PTT false
       ▼
READY (no transmit authority)
       │ deterministic autonomous policy grant
       ▼
AUTHORIZED (candidate/config/expiry bound)
       │ supported client session begins
       ▼
ACTIVE_SESSION ── complete/fault/timeout/abort ──► ABORTING
                                                      │ PTT released
                                                      ▼
                                                SAFE / RECEIVE_ONLY
```

Any invalid transition is rejected and logged. Local policy automatically
authorizes `READY → AUTHORIZED` when the installed envelope and live
interlocks pass; the planner cannot manufacture a grant or expand its scope.

Within `ACTIVE_SESSION`, the PTT arbiter separately validates each key-up
against the permit, session, lease, readback, watchdog, and local key-up
budget. Key-down is always allowed.

## 5. Authorization model

Before activation, the station owner installs a signed operating profile with
permitted bands/frequencies/channels, modes, CAT controls, maximum power,
session/duty limits, clients, geographic/station identity, message scope,
retry budget, and source-quality thresholds. Once the commissioning and
qualification gates pass, autonomous bounded operation is the only live
operating mode: policy grants eligible sessions automatically. Missing or
invalid profile data prevents autonomous startup; no runtime human approval
is requested.

CAT tool availability is capability-driven and configured in that profile.
The AI can read the backend's full control catalog and request any enabled
setting without per-action approval. Radio-specific extensions require a
registered schema, side-effect class, and qualification before the profile is
activated. Destructive/service-level or unclassified commands are denied by
default unless a specific autonomous procedure has been qualified and
authorized in advance.

Each grant is bound to one gateway/channel, modem/client, requested power
ceiling, prepared radio-state hash, message/queue scope, expiry, and policy
revision. Any mismatch revokes the grant.

The PTT arbiter is the only software path that asserts PTT. The modem/client
may request keying through it; the AI-facing `radio_ptt_request` is an explicit
PTT control surface and may do so only with an active session ID and one-use
permit. Local policy issues permits automatically after revalidating the
active session.
The arbiter enforces locally set maximum continuous key-up duration, total
session duration, duty cycle, and cooldown; callers cannot supply higher
limits. At the continuous-key-up limit, the arbiter releases PTT; any further
key-up requires a newly issued permit and revalidation. Key-down
and session abort do not require a permit. A separate low-power/test-keying
mode, if implemented, requires a qualified dummy-load or shielded test
configuration and a profile explicitly installed for commissioning; it is
never available as a routine runtime human control.

## 6. Security controls

- Bind rigctld and MCP to loopback by default.
- Treat prompts, external spots, gateway metadata, and model-generated tool
  arguments as untrusted input.
- Validate all identifiers and numeric ranges; reject unknown enum values.
- Use a per-installation local policy profile with owner-controlled permissions.
- Keep credentials and Winlink account data out of prompts, tool descriptions,
  and logs.
- Apply network timeouts, rate limits, bounded retries, and cache limits to
  remote data sources.
- Record policy denials and state-changing operations, while avoiding message
  body capture.
- Protect audit history against casual modification; document deletion/retention.
- For remote MCP, require an authenticated channel and operator-managed
  authorization; local trust is not assumed from network location alone.

## 7. Reliability, freshness, and control stability

### 7.1 Freshness

Each input has a source-specific maximum age (TBD): RMS availability,
propagation forecasts, third-party spots, radio readback, and modem state have
different validity windows. Start-session validation requires a fresh radio
readback and gateway channel snapshot. A missing source degrades confidence;
it does not become a favorable default.

### 7.2 Anti-oscillation

The ranker may continuously update suggestions, but configuration changes are
event-driven and gated. Controls include:

- minimum channel dwell time;
- minimum improvement margin (`hysteresis`);
- switch cost accounting;
- per-gateway retry budget and cooldown;
- session ownership lock;
- defer switching while client/modem reports busy;
- agent-side decision/event notification and audit, without a human approval
  dependency.

All numeric values remain unallocated until measured with station hardware and
operating patterns.

### 7.3 Failure handling

Adapter timeouts must not block the safety loop. Session supervision and the
PTT watchdog run independently from AI/provider calls. On an uncertain PTT
state, the controller assumes transmission may be active, executes
abort/release, and latches transmit inhibit. It may resume autonomously only
after independent readback and policy checks establish a safe state; otherwise
the station remains in a faulted receive-only state pending offline
maintenance. Recovery is idempotent and a replacement session starts only
after fresh ranking and a new automatic policy grant.

## 8. Operational checklist

Before activating autonomous service, commissioning must establish:

- local band/mode/power configuration matches license and station setup;
- the radio's CAT/PTT controls and timeout behavior have been tested;
- the modem/client audio and PTT routes are identified;
- the human-only emergency stop is independent and physically accessible;
- a dummy/load or shielded test has validated session boundaries where
  feasible;
- logs, privacy, and retention settings are understood;
- the signed autonomous profile, message scope, and authority ceilings are
  correct; and
- telemetry/audit can be observed without exposing runtime actuator controls.

After activation, the only human runtime actuator is emergency stop. Routine
channel selection, configuration, authorization, retry, recovery, and
session management require no operator intervention.
