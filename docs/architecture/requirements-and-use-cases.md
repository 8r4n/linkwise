# Linkwise requirements and use cases

**Document ID:** LINKWISE-SRS-001  
**Version:** 0.1, proposed  
**Status:** Draft; requirements require operator and implementation review

The requirements below express intended system behavior, not implemented
capability. “TBD” values are intentionally unallocated and must be measured or
selected before qualification.

## 1. Actors

| Actor | Description |
| --- | --- |
| Station owner / commissioning authority | Installs and qualifies station profile before deployment; during runtime has read-only telemetry and the independent emergency stop only |
| AI planner | Autonomous agent using Linkwise tools to manage CAT/PTT, candidate selection, sessions, monitoring, and recovery within its installed authority |
| Link manager | Deterministic Linkwise service orchestrating candidate, radio, modem, and client state |
| Radio | Transceiver controlled through a supported Hamlib backend |
| Modem | Existing VARA HF or future Linkwise modem |
| Winlink client | Client responsible for message queue and RMS session protocol |
| RMS gateway | Winlink radio message server/channel endpoint |
| Propagation services | VOACAP, PSK Reporter, DX Maps, GIRO, and optionally KeepTrack |

## 2. Goals and exclusions

### Goals

- Maintain a fresh, useful set of eligible Winlink gateway choices.
- Autonomously optimize message transfers for delivery probability, time, and
  resource cost using preferences installed in the station profile.
- Let the AI inspect and use the full configured CAT/PTT and link-management
  surface, with local policy enforcing its pre-authorized operating envelope.
- Preserve modem interoperability with VARA HF as a separate engineering goal.
- Keep automatic reconfiguration stable, all RF actions auditable, and the
  human runtime control surface limited to independent emergency stop.

### Exclusions for initial release

- Simultaneous RF sessions on one transceiver.
- AI-generated arbitrary CAT commands, shell commands, or unbounded/unscoped
  PTT control. Bounded PTT requests through local policy are in scope.
- Guaranteed propagation prediction or causal reentry-to-Es prediction.
- Transmission outside locally configured autonomous authorization and safety
  limits.
- Replacing the Winlink CMS, routing system, or RMS gateway software.
- Claiming specific VARA waveform/protocol behavior before reference validation.

## 3. Functional requirements

Priority: `P0` safety/core; `P1` initial useful operation; `P2` future capability.
Verification: `I` inspection, `A` analysis, `D` demonstration, `T` instrumented test.

| ID | Priority | Requirement | Verification |
| --- | --- | --- | --- |
| OVR-F-001 | P0 | The system shall expose current radio state, including every supported frequency, mode, power, VFO, and PTT field, with per-field unsupported/error status. | T |
| OVR-F-002 | P0 | The system shall mediate every radio configuration write through a single policy-controlled command path. | I,T |
| OVR-F-003 | P1 | The system shall import gateway identity, channel frequency/mode, service eligibility, location, status, last-report time, and source retrieval time. | T |
| OVR-F-004 | P1 | The system shall reject a candidate whose channel data is stale beyond a configured maximum age of TBD. | T |
| OVR-F-005 | P1 | The system shall rank eligible gateways using path evidence, service availability, antenna fit, historical outcomes, user objective, and switching cost. | A,T |
| OVR-F-006 | P1 | The system shall return a machine-readable ranking explanation with evidence provenance, timestamps, uncertainty, and exclusion reasons. | T |
| OVR-F-007 | P0 | The system shall serialize radio control with an exclusive lease and reject writes from non-owner sessions while leased. | T |
| OVR-F-008 | P0 | The system shall verify radio readback after each configuration transaction before marking the radio prepared. | T |
| OVR-F-009 | P0 | The system shall prevent retuning while PTT is asserted or a modem/client reports an active transfer, except for emergency abort. | T |
| OVR-F-010 | P1 | The system shall expose read-only telemetry to humans and provide the autonomous agent with distinct observation, non-transmitting preparation, and session-control capabilities. | I,T |
| OVR-F-011 | P0 | The system shall automatically issue transmit authorization only when the installed autonomous policy and current interlocks pass; no per-session operator approval shall be required. | D,T |
| OVR-F-012 | P0 | The system shall terminate authorization when its expiry, session time budget, lease, or safety watchdog expires. | T |
| OVR-F-013 | P1 | The system shall monitor modem/client session state and record completion, failure, abort, duration, and transfer metrics when available. | T |
| OVR-F-014 | P1 | The system shall update gateway outcome statistics from completed attempts without treating prediction-only data as session outcomes. | A,T |
| OVR-F-015 | P2 | The system shall collect propagation observations from configured providers while retaining source-specific evidence classes. | T |
| OVR-F-016 | P2 | The system shall allow reentry events to be stored as research covariates without affecting transmission policy by default. | I,T |
| OVR-F-017 | P0 | During runtime, the only human actuator shall be an emergency-stop control independent of model, MCP, network, and application availability; activation shall force PTT release/inhibit and latch the station safe. | D,T |
| OVR-F-018 | P0 | The system shall autonomously start, monitor, recover, and retry sessions within its installed authority; no runtime human approval or routine control shall be required or exposed. | D,T |
| OVR-F-019 | P0 | The system shall expose explicit PTT key-up/key-down control only through a local arbiter; key-up requires an active authorized session, exclusive lease, verified configuration, and locally capped duration/duty cycle, while key-down is always allowed and idempotent. | I,T,D |
| OVR-F-032 | P1 | The system shall expose every control operation reported as supported by the configured radio backend through capability-discovered typed tools, including readback and explicit unsupported/denied results; device-specific extensions shall be schema-registered and policy-classified. | I,T,D |
| OVR-F-033 | P0 | The autonomous AI service shall be able to invoke radio-control, PTT, session, and recovery operations through MCP within its installed authority; the system shall serialize, authorize, audit, and verify those actions without exposing direct CAT/PTT sockets or bypass paths. | I,T,D |
| OVR-F-020 | P0 | Loss of session ownership, permit expiry, watchdog timeout, abort, or a detected control fault shall request PTT release and record whether release was confirmed. | T |
| OVR-F-021 | P1 | The inventory service shall maintain separately counted unique gateway stations and channel records, with filters for protocol, band, service access, station-profile eligibility, source age, and report-age category. | T |
| OVR-F-022 | P1 | The system shall persist deduplicated session attempts and client-observed transfer events, distinguishing no-connect, RF-connected, CMS/client-accepted transfer, and recipient delivery when that final confirmation is actually available. | T |
| OVR-F-023 | P1 | The system shall report link inventory and performance summaries with an as-of time, source/snapshot identity, time window, numerator/denominator, and coverage or stale-data warnings. | T |
| OVR-F-024 | P2 | The system shall expose aggregate Winlink outbox/queue counts only through a client adapter with a supported interface; unsupported queue access shall be explicit and shall not trigger GUI scraping. | I,T |
| OVR-F-025 | P1 | A simulation harness shall run the link manager and agent against the same typed tool contracts as deployment, using simulated adapters and virtual time. | T |
| OVR-F-026 | P1 | Every simulation shall be reproducible from a versioned scenario manifest and recorded random seeds, with immutable action/result traces. | T |
| OVR-F-027 | P1 | The evaluated agent shall not receive latent RF truth, future events, evaluator-only outcomes, or seeds that reveal future scenario state. | I,T |
| OVR-F-028 | P1 | The evaluator shall compare the agent with declared simple baselines and an explicitly labeled oracle upper bound on paired scenario realizations. | A,T |
| OVR-F-029 | P1 | Simulation results shall separately report eligible inventory, attempted sessions, connections, and client-confirmed transfers; prohibited safety events invalidate a run. | A,T |
| OVR-F-030 | P2 | Modem-in-loop channel tests shall identify the channel model/version and parameters, and shall compare applicable cases with documented HF modem test recommendations. | I,T |
| OVR-F-031 | P2 | A hardware-in-loop test profile shall inhibit radiated transmission and verify PTT, readback, watchdog, and safe-stop behavior on qualified hardware. | T |
| OVR-F-034 | P1 | The system shall persist collection runs, typed source observations, forecast runs, inventory snapshots, station-profile revisions, analysis snapshots and their exact input references, candidate links and ranking entries, decisions, policy evaluations, session attempts, and lifecycle events as versioned records linked by stable identifiers, so a recommendation can be reconstructed from its exact inputs and algorithm/configuration versions. | I,T |
| OVR-F-035 | P1 | The evidence collector and path analyzer shall operate on bounded schedules independently of MCP polling; each collection run shall report provider status, timing, coverage, and warnings, and provider failure or partial response shall preserve the last valid data as stale without representing missing data as zero. | T |
| OVR-F-036 | P1 | MCP history and analysis tools shall return bounded provenance-bearing summaries with source/model versions, exact analysis and ranking identifiers, time windows, counts, denominators, coverage, freshness, and uncertainty; the AI conversation shall not be the system of record for evidence or outcome statistics. | I,T |
| OVR-F-037 | P2 | The system shall support deterministic replay of a candidate ranking and associated decision from immutable analysis inputs, inventory snapshot, station-profile revision, historical aggregate version, ranking entries, and ranker/policy versions, subject to source licensing and retention constraints. | A,T |
| OVR-F-038 | P1 | Each analysis input shall reference exactly one immutable target and identify its input role; corrected or revised source data, forecasts, station assumptions, or analysis shall append a new version without changing prior decisions. | I,T |
| OVR-F-039 | P1 | The system shall record explicit select, wait, and no-route decisions, with an optional selected candidate, and link each decision to zero or more distinct session attempts; every retry shall create a new attempt linked to the originating decision. | I,T |
| OVR-F-040 | P1 | Historical outcome aggregates shall be derived from deduplicated measured session and transfer events, retain the aggregation window, match dimensions, sample count, denominator, coverage, and uncertainty, and remain traceable to the underlying records. Prediction-only evidence shall not be counted as a measured outcome. | A,T |

## 4. Quality requirements

| ID | Quality attribute | Requirement / target |
| --- | --- | --- |
| OVR-Q-001 | Safety | On loss of the OLM session controller, the system shall release software-controlled PTT and enter the configured receive-only safe state within TBD ms; physical interlocks remain independent. |
| OVR-Q-002 | Freshness | Each datum shall include observation/retrieval time and maximum-valid-age policy; stale data shall not silently appear current. |
| OVR-Q-003 | Reliability | The radio-control and policy path shall remain local and shall not require internet availability for emergency stop or safe return. |
| OVR-Q-004 | Explainability | A gateway recommendation shall include the factors, data sources, freshness, and constraints that determined its rank. |
| OVR-Q-005 | Stability | Configuration changes shall use configurable hysteresis, minimum dwell time, cooldown, and maximum retry count. Numeric defaults TBD. |
| OVR-Q-006 | Portability | The Rust system is intended for macOS, Windows, and Linux on ARM64 and x86_64; runtime/device validation is distinct from cross-compilation. |
| OVR-Q-007 | Auditability | Every state-changing request and policy decision shall have a correlation ID, timestamp, actor, old/new state, decision, and outcome. |
| OVR-Q-008 | Privacy | Message payloads shall not be retained for link optimization unless explicitly enabled; operational metadata retention is configurable. |
| OVR-Q-009 | Interoperability | A modem release shall demonstrate bidirectional transfer with the unmodified reference modem/client under controlled conditions before claiming compatibility. |
| OVR-Q-010 | Reproducibility | Given the same simulator build, scenario manifest, and seed, event ordering and non-agent outcomes shall replay deterministically. |
| OVR-Q-011 | Evaluation integrity | Scenario families and chronological traces used for final evaluation shall be held out from policy/prompt tuning; results shall include uncertainty and denominators. |

## 5. Use cases

### UC-01: Autonomously observe station and available channels

**Primary actor:** Link manager and AI planner. **Preconditions:** OLM is
running; radio, modem, and client adapters are configured. **Main flow:** A
scheduler reads adapter state, obtains a gateway snapshot, marks unsupported
and stale fields, and returns a consolidated timestamped view to the agent.
**Alternatives:** Adapter unavailable → return partial state and explicit
degraded status; stale directory → mark snapshot stale and suppress session
start until refresh succeeds. **Postcondition:** No radio setting changed.

### UC-02: Recommend a Winlink gateway

**Primary actor:** AI planner. **Preconditions:** Location/station profile and
objective are installed. **Main flow:** OLM filters channels by service,
modem/band support, current status, and local limits; gathers available path
predictions and observations; calculates expected utility; applies switching
costs and uncertainty; returns ranked candidates with reasons. **Alternative:**
Insufficient evidence → return a low-confidence shortlist or no candidate,
never fabricated reliability. **Postcondition:** No radio setting changed.

### UC-03: Prepare the radio for a selected gateway

**Primary actor:** AI planner. **Preconditions:** Candidate remains
eligible; radio lease is available; no active session/PTT. **Main flow:** OLM
checks local policy, reserves the radio lease, applies the full selected CAT
configuration, reads back supported fields, validates audio/PTT readiness, and
reports READY or a specific failure. **Alternative:** A mismatch triggers
safe rollback; the agent automatically refreshes evidence, selects a safe
alternative or waits, and never transmits on uncertain configuration.
**Postcondition:** Configuration is verified; no RF transmission occurs.

### UC-04: Autonomously start a bounded Winlink session

**Primary actor:** AI planner and link manager. **Preconditions:** UC-03
READY; fresh channel; supported client/modem; channel-clear assessment passes;
installed autonomous policy and interlocks pass. **Main flow:** Local policy
automatically issues a single-session token bound to the gateway, frequency,
power ceiling, client, scope, and expiry; OLM starts the session and monitors
PTT, modem, client, watchdog, and transfer progress. **Alternative:** Expiry,
PTT duration limit, unexpected frequency, lost lease, or protection event
terminates the attempt; the system records it and autonomously recovers or
selects another candidate within retry budgets. **Postcondition:** Session
result and audit are recorded without per-session human approval.

### UC-05: Recover after failed connection

**Primary actor:** Link manager. **Preconditions:** A session has failed or
timed out. **Main flow:** OLM releases PTT, records failure category, applies
cooldown/retry policy, refreshes stale state if needed, re-ranks alternatives,
and selects the next safe action automatically. It must not retune if the cause
or channel state is unknown; it refreshes/observes or enters a latched safe
state. Each retry receives a new policy grant. **Postcondition:** Radio is
safe, the result is retained, and recovery proceeds without routine human
intervention.

### UC-06: Maintain the gateway opportunity set

**Primary actor:** Link manager. **Preconditions:** Monitoring enabled.
**Main flow:** At configured intervals OLM refreshes RMS channels and passive
propagation observations; it updates candidate freshness and route scores;
applies hysteresis; and autonomously chooses the next eligible channel when a
material improvement is actionable. It does not retune during PTT or an active
transfer. **Postcondition:** A fresh candidate set is maintained and
transmissions remain within installed authority.

### UC-07: Audit autonomous decisions and sessions

**Primary actor:** Audit service; read-only reports are available for
maintenance/review outside live station control. **Main flow:** Retrieve the
selected candidate, evidence records, policy revision/grant, configuration
readback, recovery actions, and session outcome by correlation ID.
**Postcondition:** Decisions can be reconstructed without storing message
content; audit access does not provide runtime actuator control.

### UC-08: Compare reentry periods with 10 m openings

**Primary actor:** Researcher. **Main flow:** Import reentry estimates as a
separate time series; align them with independently collected 10 m observations
and session attempts; compare against control intervals and report limitations.
**Safety condition:** Reentry data shall not be an authorization signal.

### UC-09: Request bounded PTT key-up or release

**Primary actor:** Session controller/AI planner. **Key-up preconditions:** A live session owns the radio lease; a
single-use transmit permit is valid and bound to the current configuration;
the radio is in policy-approved state; the per-key-up and session budgets are
not exhausted. **Main flow:** The requester submits `radio_ptt_request` with
`key_up`, the session ID, and permit ID; the PTT arbiter revalidates all
conditions, keys through the configured qualified driver, starts/refreshes its
watchdog, and returns observed state. The request cannot specify its own
maximum duration. **Key-down flow:** Any authorized station actor may request
`key_down` without a transmit permit; it is idempotent and preempts a pending
key-up. **Failure:** On timeout, lease loss, permit expiry, or uncertain PTT
state, the arbiter attempts release, latches transmit inhibit, and autonomously
aborts. **Postcondition:** Every transition/readback is audited; no replacement
transmission starts until deterministic checks establish safe state and policy
permits resumption. The human emergency stop remains independent.

### UC-10: Track the Winlink gateway and channel inventory

**Primary actor:** Link manager; AI planner queries the resulting summary.
**Main flow:** A background inventory service refreshes a configured,
documented source or imports a supported client snapshot; validates provenance
and freshness; deduplicates station and channel identities; applies local
protocol, band, service-code, and station-profile filters; and publishes
timestamped counts and deltas. The agent can request a refresh, summary, or
per-gateway channel detail. **Alternatives:** No supported source, inaccessible
private service code, or invalid records → preserve the last snapshot as
stale, report coverage/error, and do not claim a complete list. A refresh never
changes radio configuration or starts a session. **Postcondition:** Current
inventory snapshot and its source age are queryable.

### UC-11: Monitor connection and message-transfer outcomes

**Primary actor:** Session controller; the autonomous agent and read-only human telemetry view may query history or metrics.
**Main flow:** Record each unique attempt and lifecycle transition, associate
the selected gateway/channel and radio/modem configuration, then ingest
client-reported transfer events. Deduplicate using local session/event IDs and
Winlink MID when exposed. Report RF session connection separately from message
transfer acceptance and recipient delivery. Return counts/rates with a
selected time window, denominator, and data coverage. **Alternatives:** Client
does not expose a supported queue/transfer interface → report it as
unsupported; do not infer message counts from public traffic maps or retain
message content. **Postcondition:** Failed and successful attempts both
contribute to the auditable history.

### UC-12: Evaluate link-management decisions in a simulated RF environment

**Primary actor:** Developer/evaluator. **Preconditions:** A versioned
scenario, simulator build, agent/tool configuration, and evaluation objective
are recorded. **Main flow:** The harness resets a virtual station, exposes only
agent-visible observations through the production tool schemas, advances
virtual time, and records each decision and outcome. It repeats the same seeded
scenario for the agent, declared baselines, and a truth-informed oracle upper
bound; then computes delivery utility, latency, attempts, churn, regret,
calibration, tool errors, and safety outcomes with uncertainty. **Alternative:**
Missing model calibration or unsupported modem fidelity → label results as
scenario-model evidence, not real-world prediction or modem validation.
**Postcondition:** A provenance-complete trace and report are available for
replay; the evaluated agent cannot inspect latent RF truth.

## 6. Requirement-to-component allocation

| Requirement family | Owning component(s) |
| --- | --- |
| OVR-F-001, 007–009 | Hamlib adapter, lease manager, session controller |
| OVR-F-003–006, 014–016, 034–037 | Gateway/evidence adapters, normalizer, evidence store, path analyzer, ranker, MCP history interface |
| OVR-F-010–012, 017–018 | MCP interface, policy engine, watchdog, read-only telemetry, independent emergency-stop circuit |
| OVR-F-019–020 | PTT arbiter, qualified PTT driver, policy engine, watchdog, audit store |
| OVR-F-021 | Winlink inventory provider, normalizer, snapshot store, link ranker |
| OVR-F-022–024 | Session controller, client adapter, link-history aggregator, privacy controls |
| OVR-F-025–029, OVR-Q-010–011 | Simulation harness, scenario runner, evaluator, trace store |
| OVR-F-030 | Modem-in-loop channel adapter and qualification harness |
| OVR-F-031 | Hardware-in-loop harness, PTT arbiter, qualified hardware fixture |
| OVR-F-013 | Modem/client adapters, session controller, audit store |
| OVR-Q-001–008 | Policy, local runtime, storage, adapters, operations |
| OVR-Q-009 | Modem implementation and interoperability qualification |

## 7. Qualification strategy

For an eventual release, requirements will map to inspection, analysis,
demonstration, or instrumented test. Initial evidence should include a dummy
rigctld backend, simulated stale/busy channels, fake modem/client adapters,
fault injection for lost control/PTT/timeouts, and deterministic replay of
gateway/evidence snapshots. Real-transmitter qualification is a separate
controlled activity and is outside the current software scaffold.

The proposed RF simulator and its fidelity gates are detailed in
[rf-agent-simulation-design.md](rf-agent-simulation-design.md). A fast
discrete-event decision simulator does not establish modem interoperability;
baseband modem-in-loop and shielded/conducted hardware-in-loop tests answer
different questions and must be reported separately.
