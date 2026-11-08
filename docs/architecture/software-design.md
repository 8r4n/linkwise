# Linkwise software design

**Document ID:** LINKWISE-SDD-001  
**Version:** 0.1, proposed  
**Status:** Draft design baseline; interfaces are not implemented

## 1. Purpose and design scope

This Software Design Description (SDD) turns the [system architecture](system-architecture.md)
and [MCP interface proposal](mcp-interface-design.md) into implementable
software boundaries. It specifies which layer owns each decision, how requests
cross those boundaries, how stateful radio operations are coordinated, and
how the design fails safely. All diagrams are authored in PlantUML under
[`diagrams/`](diagrams/); update those sources rather than hand-editing
rendered output.

Linkwise is proposed as a local-first, AI-assisted Winlink link manager with a
separable VARA-compatible modem research effort. The design does not assume a
particular Winlink client automation API, VARA command behavior, radio PTT
capability, or propagation accuracy. Adapters must advertise capabilities;
unsupported operations return an explicit result rather than being emulated
silently.

### Design principles

1. **The model can operate the station through an explicit control plane.**
   Linkwise exposes the complete capability-reported CAT control set and
   explicit PTT tools to the AI host through MCP. The model never connects to
   a device socket or writes arbitrary protocol bytes itself; each request is
   interpreted by the radio adapter and checked against the active station
   profile, authority mode, lease, and safety invariants.
2. **Safety and domain decisions are deterministic.** Eligibility,
   authorization, resource locking, interlocks, and PTT limits are ordinary
   versioned code. A model recommendation is never a permit.
3. **Adapters isolate outside protocols.** Hamlib, modem, Winlink client,
   propagation feeds, persistence, and simulated devices implement narrow
   ports owned by the application/domain.
4. **Every observation is qualified.** Values carry source, observation time,
   validity, provenance, and quality. Unknown is not zero; forecast is not
   measurement; listing is not reachability.
5. **Mutations are serialized and auditable.** A single station command
   coordinator owns a radio lease; every state-changing operation has an
   idempotency key, correlation ID, expiry, and result event.
6. **The safety loop is independent of AI and network latency.** PTT release,
   session deadlines, and abort handling do not wait on model inference or
   remote evidence providers.

## 2. Runtime boundaries and layer responsibilities

The layers and their dependency direction are shown in
[`layered-architecture.puml`](diagrams/layered-architecture.puml).
Dependencies point inward to application/domain contracts. An adapter may
depend on a port definition; the domain must not depend on a particular
adapter, MCP SDK, radio backend, database, or LLM provider.

| Layer | Owns | Must not do |
| --- | --- | --- |
| AI host / operator boundary | Autonomous goal selection, explanation, and invocation of the full configured CAT/PTT and link-management MCP surface | Treat its memory as authoritative station state; change the installed station profile, access hardware directly, or bypass Linkwise policy |
| Inbound interface | MCP discovery, schema validation, identity/permission mapping, request limits, response envelopes | Implement link scoring, safety policy, or driver protocols |
| Application/use cases | `RefreshInventory`, `RankCandidates`, `PrepareCandidate`, `RequestAuthorization`, `StartSession`, `AbortSession`; orchestration and transaction boundaries | Trust a previous tool read as authorization; let long network calls block safety work |
| Domain core | Candidate eligibility, scoring, station/session state machines, policy decisions, limits, leases, event semantics | Perform I/O, read environment variables as policy, invoke an LLM |
| Outbound ports | Stable contracts for radio, PTT, modem, Winlink, evidence, persistence, clock, and notifications | Encode provider-specific wire formats |
| Infrastructure adapters | Hamlib/rigctld, VARA or Linkwise modem, supported Winlink client, VOACAP/data providers, local storage, simulator | Bypass domain policy or claim unsupported capabilities |
| Device / external systems | Transceiver, audio/PTT hardware, RMS/CMS, published forecasts and observations | Be treated as transactionally reliable or fully observable |

### Process and trust boundaries

The initial product is a single operator workstation. The AI host talks to
Linkwise over local MCP stdio where possible. If Streamable HTTP is required,
bind loopback by default and authenticate any non-loopback deployment. The
Linkwise application process contains the MCP adapter, use-case services,
domain, policy/safety supervisor, and local persistence. Device adapters may
be in-process libraries or separate local processes, but their transport is
not an authorization boundary. `rigctld`, modem control ports, audio routes,
and client automation interfaces are never exposed to the model.

Keep the optional model/provider process and remote evidence providers
outside the trusted control boundary. A model/provider outage disables new
model-dependent plans and triggers the deterministic fallback policy or a
safe autonomous pause; it never disables emergency stop or the PTT release
supervisor. The simulator implements the same ports with
virtual devices and a virtual clock, but evaluator-only channel truth is not
available through production-facing MCP tools.

## 3. Component design

The component dependency view is in
[`layered-architecture.puml`](diagrams/layered-architecture.puml); the
principal interfaces and data types are in
[`interfaces-and-domain.puml`](diagrams/interfaces-and-domain.puml).

### 3.1 Inbound API and command dispatch

`McpServer` is a transport adapter. It maps versioned MCP tools—including
capability discovery, radio-control reads/writes, and explicit PTT requests—to
typed application commands and maps results to a common response envelope. It
performs syntactic checks (required fields, enum values, size limits); the
application/domain repeat semantic checks using current state. MCP annotations
are advisory only. Internal command handlers attach an authenticated local
principal, `request_id`, `correlation_id`, `idempotency_key`, deadline, and
requested permission class. The principal is derived from host/process
configuration; the autonomous service identity has only the installed
operating scope and never derives authority from a model-supplied argument.
During autonomous runtime the operator interface is telemetry plus one
independent emergency-stop control; it offers no routine approve, tune, start,
or recovery action. Station profiles are installed and validated before the
autonomous service is enabled.

Use-case handlers expose both goal-oriented operations and capability-driven
radio controls. Candidate workflows take a `CandidateId`, station objective,
and optional preference set; the candidate is reloaded from current inventory
before writes. Advanced radio operations use a typed `ControlId` and validated
value, or a registered vendor-extension identifier with a schema—not an
arbitrary shell command or unvalidated CAT byte string. This allows the agent
to use the radio's full reported control surface while preserving a single
auditable adapter/policy path.
MCP result resources are snapshots and are never capabilities. The only
capabilities are opaque, short-lived prepared handles, authorization grants,
and one-use transmit permits issued by local services.

### 3.2 Application services

The application layer coordinates operations with explicit deadlines and
cancellation. Its key handlers are:

- `RefreshInventory`: ask the directory/provider adapter for source data,
  normalize/deduplicate gateway and channel identities, persist an immutable
  inventory snapshot, and publish a change event. It never tunes or keys the
  radio.
- `CollectEvidence`: scheduled bounded workers poll only configured providers,
  validate and normalize records, deduplicate by provider identity/revision,
  preserve source and observation timestamps, and append immutable evidence
  records. Provider timeouts and failures update health state without blocking
  station or safety work.
- `AnalyzePaths`: load a pinned set of evidence and inventory snapshot IDs,
  station profile, VOACAP/P.533 results, and measured session outcomes; run a
  deterministic versioned analyzer and persist immutable per-path analysis
  snapshots with inputs, features, uncertainty, coverage, and score components.
  This runs on evidence changes and planning cadence, independently of MCP
  polling.
- `RankCandidates`: load station profile, current inventory, propagation and
  observation evidence, and measured session outcomes; construct a
  deterministic feature set and return ranked candidates with reasons,
  freshness, uncertainty, and exclusions.
- `ControlRadio`: acquire or validate the exclusive station lease, resolve a
  requested control against the backend capability catalog and station
  profile, apply a typed value or registered extension operation, perform
  readback where supported, and append an audit event. This gives the AI the
  full configured CAT surface; candidate preparation is a convenience use
  case built on the same control path.
- `PrepareCandidate`: acquire the exclusive station lease; revalidate the
  candidate and station profile; ensure modem/client idle and PTT released;
  perform an ordered radio configuration transaction; verify readback; then
  issue an opaque prepared handle bound to that exact state revision and
  lease. The station actor retains the lease through the short prepared
  interval; session start adopts it, while expiry, cancellation, or failure
  releases it.
- `RequestAuthorization`: ask local deterministic policy to evaluate the
  autonomous operating envelope, objective, queue/message scope, and limits.
  If conditions pass, policy issues a bounded grant automatically; the model
  can neither mint nor widen it. No per-session human approval exists in the
  autonomous runtime profile.
- `StartSession`: revalidate handle, automatically issued local-policy grant,
  lease, inventory age, radio
  readback, client/modem readiness, and interlocks immediately before asking
  the supported Winlink client to begin one bounded session. Session grants
  are automatic within the installed envelope; no interactive approval is
  requested while autonomous operation is active.
- `ObserveSession` / `AbortSession`: collect state and progress without
  conflating gateway contact, modem connection, client acceptance, and
  recipient delivery; abort is idempotent and prioritizes PTT release.

Operations that wait on the network are asynchronous and must not hold a
database transaction or block the safety supervisor. An
operation deadline cancels work and produces a terminal event; cancellation
does not imply that a hardware write was rolled back, so the caller must
observe/read back the resulting device state.

### 3.3 Evidence persistence and retrieval

Persistence is a system of record, not conversational memory. The repository
stores append-only `CollectionRun`, typed `SourceRecord`, `ForecastRun`,
`InventorySnapshot`, `StationProfileRevision`, `AnalysisSnapshot`,
`AnalysisInput`, `HistoricalOutcomeAggregate`, `CandidateLink`, `RankingEntry`,
`Decision`, `PolicyEvaluation`, `SessionAttempt`, and `SessionEvent` records.
Each derived record references exact upstream IDs and versions. An
`AnalysisInput` identifies exactly one input target (source record, forecast,
inventory snapshot, station-profile revision, model/config revision, or
historical aggregate) and its role in the analysis. Ranking entries belong to
an immutable candidate ranking; decisions refer to that ranking and may
select a candidate, wait for better evidence, or report no route. A decision
may lead to multiple distinct session attempts, each with its own lifecycle
events and separately recorded connection and transfer outcomes.

Corrections, updated forecasts, changed station assumptions, and re-analysis
append new versions; they do not mutate the evidence behind prior decisions.
Collection runs record provider request/result status, timing, coverage, and
warnings so partial failure cannot be mistaken for an empty observation set.
Repeated deliveries are deduplicated by provider identity and source event
identity/content hash while preserving revision history. Current provider
health and latest-candidate pointers may be materialized views rebuilt from
events. Historical aggregates are versioned derived inputs with explicit
time windows, match dimensions, sample counts, denominators, and uncertainty;
they never replace the individual attempts and transfer events from which
they were computed.

`GetSimilarSessions` and `GetHistoryMetrics` query deduplicated outcomes by
path, gateway, frequency, season/time window, mode, and station profile as
requested. Results include selected range and match criteria, sample count,
numerator/denominator, coverage, and uncertainty. The ranker applies
shrinkage or another declared sparse-data policy and exposes its version;
the AI may interpret the returned summary but does not own persistent counts
or trusted aggregates. Raw provider payload retention is governed by each
source's terms; retain permitted normalized fields, attribution, stable IDs,
and checksums/references where raw retention is not allowed. Session storage
excludes message bodies and credentials by default. SQLite/WAL remains an
initial adapter choice behind repository interfaces, with schema migrations
and replay/export support.

### 3.4 Domain and policy services

Domain services are pure or stateful logic over explicit inputs:

- `EligibilityEvaluator` applies protocol/service/band/mode/station-profile
  rules and excludes stale, unsupported, or incompatible channels.
- `EvidenceAssembler` preserves evidence classes and provenance, then emits
  a bounded candidate evidence view. It does not turn a VOACAP probability,
  spot, ionosonde record, or reentry feature into a fact about current
  reachability.
- `CandidateRanker` computes a deterministic baseline score/expected utility
  for a stated message objective, with source age, confidence, uncertainty,
  station fit, empirical outcomes, time cost, retries, and channel-switch cost
  visible as separate score components. AI may explain or propose a
  preference; it cannot rewrite the score or eligibility result.
- `StationPolicy` validates the local control catalog/extension class,
  allowlist, power/mode/frequency bounds, authorization, session/airtime
  budgets, channel-busy result where available, and station operating mode.
- `SessionAggregate` and `RadioLease` enforce legal state transitions and
  mutual exclusion. `PttArbiter` is a separate, narrowly exposed actuator
  service; it does not trust a session ID by itself.
- `OutcomeClassifier` records distinct outcomes (attempted, gateway contact,
  modem connected, transfer accepted by client, later delivery confirmation
  when available). It never overstates what a source confirms.

Keep ranking policy injectable and versioned for replay, simulation, and
shadow evaluation. The same candidate snapshot and ranker version should
produce the same ordering, subject only to explicitly versioned time-derived
features.

### 3.5 Outbound adapters and integration seams

| Port / adapter | Contract and boundary behavior |
| --- | --- |
| `RadioControlPort` / Hamlib adapter | Enumerate all backend-reported capabilities and state; read/set each supported typed control (frequency/VFO, mode/filter, power, antenna, levels, split, memory, functions, and backend extensions as supported); return per-control support, bounds, and readback. Translate rigctld replies/errors; never bypass policy or own PTT authorization. |
| `PttControlPort` / PTT arbiter + qualified driver | Expose PTT observation, authorized assert, and unconditional idempotent release. Assert independently checks session or approved test context, lease, config revision, permit, limits, route health, and watchdog. Report requested and observed state separately. |
| `ModemSessionPort` / VARA adapter | Observe lifecycle and link metrics; start/stop only through supported documented interface. Every keying request routes to PTT arbiter; model cannot sequence modem bursts. |
| `WinlinkClientPort` / client adapter | Expose only documented inventory/queue/session operations supported by that client. Return `Unsupported` for absent features; do not silently scrape a GUI. A client event is not proof of recipient delivery. |
| `GatewayDirectoryPort` | Fetch source version, records, status/report timestamps, pagination and warnings. Normalize identity without collapsing multiple channels under one gateway. |
| `PropagationProviderPort` | Accept explicit path, time, frequency, antenna/station and model settings; return a typed forecast with model/version/input provenance and uncertainty. |
| `ObservationProviderPort` | Return measured/reported observations with source, location/footprint, report age and quality; do not promote them to transmit authorization. |
| `HistoryRepository` / `AuditWriter` | Append deduplicated events and immutable policy/action decisions; query aggregates. Store no message bodies by default. Audit writes are part of command completion semantics. |
| `Clock` / `Scheduler` | UTC wall time for provenance plus monotonic time for deadlines, leases and watchdogs. The simulator substitutes virtual time. |

Adapters expose health and capability snapshots at startup and after reconnect.
They use bounded timeouts, backoff with jitter, response-size limits, and
explicit error classes. A remote provider outage returns partial evidence,
not a fabricated healthy record. Adapter callbacks are converted to domain
events on a bounded queue; queue overflow is observable and safety-critical
events (abort/PTT release) use a separate priority path.

## 4. Software interface contracts

### 4.1 Common envelope and value semantics

The MCP interface proposal defines the external JSON envelope. Internally,
commands and results should be Rust types, not unvalidated JSON maps. At a
minimum, each request carries:

```text
RequestContext {
  request_id: UUID,
  correlation_id: UUID,
  idempotency_key: String,
  principal: Principal,
  deadline_monotonic: Instant,
  policy_revision: PolicyRevision
}
```

External observations use a shared envelope:

```text
Observed<T> {
  value: T,
  source: SourceId,
  source_observed_at: Option<UtcDateTime>,
  retrieved_at: UtcDateTime,
  valid_until: Option<UtcDateTime>,
  provenance: ProvenanceRef,
  quality: Quality,            // measured, reported, modeled, inferred, unknown
  warnings: Vec<Warning>
}
```

Use `Option`/explicit unknown and capability enums for absent state; do not
coerce unsupported PTT readback, unavailable SNR, or missing queue data to
`false`, `0`, or an empty successful result. Convert provider time to UTC at
the adapter boundary and use a monotonic clock for elapsed duration.

### 4.2 Command semantics

- Read operations may be retried within their deadline and return a snapshot
  ID when derived from stored data.
- Inventory refresh is externally read-only but can consume network/API
  capacity; apply source-specific rate limits and cache/TTL policy.
- Mutating requests require idempotency. A duplicate key with identical
  principal and payload returns the original operation/result; reuse with a
  different payload is rejected.
- Radio preparation is a transaction-like sequence, not an atomic device
  operation. Serialize through a lease, perform writes in a known order,
  read back each supported field, and on partial failure attempt a bounded
  restore to the last known safe profile. If restoration cannot be verified,
  invalidate the prepared handle, release PTT, and latch transmit-inhibit.
  Autonomous recovery may resume only after readback and interlocks prove a
  safe state; otherwise the service remains faulted until maintenance outside
  runtime operation.
- Authorization and permit tokens are random opaque references stored only
  locally, one-use where specified, bound to candidate/config/policy/session,
  short-lived, and revoked on lease loss, state change, policy revision, or
  expiry. Possession is necessary but is not the only check.
- Abort and key-down are idempotent safety commands that preempt normal work.
  Their response distinguishes “release requested” from “release observed.”
- CAT access is capability-complete but policy-mediated: every settable
  backend control is represented as a typed operation when possible; a
  device-specific extension must be registered with schema, side-effect
  classification, and policy requirements. No raw rigctld socket or
  model-authored command string can skip the command coordinator.

### 4.3 Principal interface signatures

The names below define semantic contracts, not a finalized Rust crate API:

```rust,ignore
type AdapterResult<T> = Result<T, AdapterError>;
type PttResult<T> = Result<T, PttError>;

trait RadioControlPort {
    async fn capabilities(&self) -> AdapterResult<RadioCapabilities>;
    async fn snapshot(&self) -> AdapterResult<Observed<RadioSnapshot>>;
    async fn get_control(&self, control: ControlId)
        -> AdapterResult<Observed<ControlValue>>;
    async fn set_control(&self, control: ControlId, value: ControlValue,
                         lease: &RadioLease)
        -> AdapterResult<RadioReadback>;
    async fn invoke_extension(&self, extension: ExtensionId,
                              arguments: TypedArguments,
                              lease: &RadioLease)
        -> AdapterResult<ExtensionResult>;
}

trait PttControlPort {
    async fn observe(&self) -> PttResult<Observed<PttState>>;
    async fn assert(&self, permit: &TransmitPermit)
        -> PttResult<PttObservation>;
    async fn release(&self, cause: ReleaseCause) -> PttResult<PttObservation>;
}

trait ModemSessionPort {
    async fn status(&self) -> AdapterResult<Observed<ModemStatus>>;
    async fn begin(&self, request: ModemSessionRequest)
        -> AdapterResult<ModemSessionId>;
    async fn stop(&self, id: ModemSessionId, cause: StopCause)
        -> AdapterResult<()>;
}

trait WinlinkClientPort {
    async fn capabilities(&self) -> AdapterResult<ClientCapabilities>;
    async fn begin_session(&self, request: ClientSessionRequest)
        -> AdapterResult<ClientSessionId>;
    async fn observe(&self, id: ClientSessionId)
        -> AdapterResult<Observed<ClientSessionState>>;
    async fn abort(&self, id: ClientSessionId) -> AdapterResult<()>;
}
```

The extension operation is available only for backend commands registered
with a versioned schema, declared side effects, and an explicit station-policy
classification. The installed autonomous profile enumerates which controls
are available; every supported operational control can be enabled without
runtime approval. Reset, firmware/service, calibration, and unclassified
operations require explicit pre-deployment qualification and profile
configuration, never per-action operator input. `radio_ptt_request`
allows the agent to request key-up or key-down, but key-up still requires a
permit and locally selected ceiling. PTT release, abort, and hardware watchdog
remain independent of the agent. The modem retains burst-timing responsibility
for normal VARA sessions; AI PTT requests serve bounded station-control or
explicitly authorized test use, not per-symbol or per-burst timing.

All async calls must be bounded by caller deadlines. The adapters must not
retain references to ephemeral MCP request objects; accepted work is copied
into an immutable command record. The final implementation may use a
synchronous interface for device calls if measurement shows that it is more
reliable, but synchronous calls still must be isolated from safety timing.

## 5. Main runtime flows

### 5.1 Evidence refresh and gateway ranking

The rank path is read-only. The application obtains an immutable inventory
snapshot, station/radio capabilities, per-path forecast, relevant observations,
and historical results; it records source errors and missing coverage. The
eligibility evaluator runs before the ranker. Returned choices carry the
snapshot ID and explanatory score components. This path never obtains a radio
lease or PTT permit. See
[`rank-candidates.puml`](diagrams/rank-candidates.puml).

Use a background scheduler for routine inventory/evidence refresh and event
ingestion. The agent is invoked on scheduled/material decision events under
configured resource limits; it autonomously ranks, prepares, starts, and
recovers sessions without waiting for an operator. It must not retune during
PTT or an active transfer. If model services fail, deterministic local policy
continues only prevalidated actions or pauses safely; it never makes an
unbounded transmit decision.

### 5.2 Candidate preparation

Preparation is a bounded, non-transmitting saga. Acquire the station lease,
check no session/PTT/busy condition blocks changes, refresh or confirm the
candidate snapshot, authorize the proposed config, apply typed radio fields,
and read them back. Create `PreparedHandle` only if the verified state hash
matches the request. The handle retains the exclusive lease through a short
ready interval; session start adopts it, and expiry/cancellation releases it.
This closes the race between preparation and start without locking the radio
indefinitely. On failure, perform the defined compensation/release path and
report any unresolved state. See
[`prepare-candidate.puml`](diagrams/prepare-candidate.puml).

The prepared handle is not transmit authorization. It is invalidated by
external knob changes, readback drift, lease loss, candidate expiry, policy
change, modem/client activity, or timeout.

### 5.3 Authorized session and transmit control

Session start rechecks all conditions; no earlier rank or prepare response
substitutes for current state. A session grant is human-approved or produced
by a preconfigured local bounded-auto policy, never by the model itself. The
Winlink client and modem own protocol sequencing. A modem keying request
passes through the local PTT arbiter, which rechecks session, lease, config,
one-use permit, continuous-key-up/duty budget, and watchdog state before
assertion. Release occurs at the local limit and on abort, timeout, fault, or
controller/lease loss. See
[`authorized-session.puml`](diagrams/authorized-session.puml) and
[`session-state-machine.puml`](diagrams/session-state-machine.puml).

### 5.4 Event capture and outcome learning

Persist append-only lifecycle events for inventory refresh, policy decision,
lease acquisition/loss, radio requested/readback state, PTT requested/observed
transition, modem state, client session state, autonomous policy grants,
abort, and emergency-stop activation.
Every event includes correlation IDs, UTC timestamps, source, adapter/policy
versions, and safe redaction. Use deduplication keys for retried adapter
events. Materialized counters are derived from events and retain numerator,
denominator, time window, source coverage, and inventory snapshot ID.

Ranking learns from confirmed measured outcomes through a separately versioned
empirical feature. Do not feed raw PTT/controller faults back as if they were
RF path failures. Retention of user-identifying telemetry is configurable;
message body, recipient, and credentials are excluded by default.

## 6. State, concurrency, and fault containment

The session state machine and a separately enforced PTT state machine are
shown in [`session-state-machine.puml`](diagrams/session-state-machine.puml).
The radio is an exclusive resource: one lease owner can prepare/start a
session; observers may read snapshots but may not mutate. Use a monotonically
increasing state revision to prevent a stale command from overwriting a
newer autonomous or external state change. An external state change invalidates preparation and
authorization rather than being “corrected” behind the operator's back. The
operator has no routine control path during autonomous runtime; a physically
independent emergency stop is the sole human actuator.

Recommended execution isolation:

- **Safety supervisor:** short, bounded, high-priority loop for PTT permit
  expiry, key-up cap, session deadline, autonomous abort, emergency-stop
  latching, and release confirmation.
- **Station command actor:** one serialized writer per physical station for
  tune/mode/power/session mutations; it owns the radio lease and revision.
- **Observation workers:** independent bounded I/O tasks for directory,
  forecast, spots, ionosonde, and client/modem state; stale/failed providers
  cannot block radio safety work.
- **Event writer:** bounded append-only persistence queue; define and surface
  behavior on disk-full/write failure. Do not acknowledge a safety-relevant
  operation as complete if the required audit record cannot be committed,
  except the physical release itself, which must proceed even when logging
  fails and raise a local alarm.
- **MCP request workers:** bounded concurrency and per-principal rate/size
  limits. Requests that mutate station state route to the station actor;
  reads may run concurrently on immutable snapshots.

Failure policy is fail-closed for permission to transmit and fail-safe for
release: lost or contradictory state can never extend a permit. If PTT release
cannot be confirmed, mark the station `Faulted`, latch transmit inhibit, and
prevent new sessions. Attempt only prequalified autonomous recovery; if the
state remains uncertain, stay receive-only until maintenance outside the
autonomous runtime. A software watchdog is not a substitute for a qualified
hardware transmit inhibit.

## 7. Persistence and information model

The logical entities and ownership links are in
[`information-model.puml`](diagrams/information-model.puml). A local
transactional store is proposed for station profile revisions, immutable
inventory/evidence snapshots, prepared handles/grants/permits, session and
transfer events, policy decisions, and audit records. A single-user local
deployment can begin with SQLite/WAL behind repository interfaces; this is a
provisional implementation choice, not a domain dependency. Keep raw source
payloads only when terms, privacy, and retention permit; persist normalized
fields plus source/version/hash to reproduce decisions.

The authoritative state of an active session is the local state machine and
live adapter readbacks, not an AI conversation or a remote cache. On restart,
all in-memory permits and prepared handles are invalid. Reconcile radio PTT,
radio configuration, modem/client activity, and unfinished session events
before enabling new actions; default to receive-only/blocked until state is
known safe. Autonomous readiness also requires a valid signed station profile,
successful adapter capability checks, valid time, and a tested emergency-stop
path. Failure of any precondition keeps transmission inhibited. The physical
emergency stop is available at all times and acts independently of the AI
host, MCP transport, and application scheduling; no other human actuator
control is exposed during autonomous operation.

## 8. Deployment and evolution

The deployment view is in [`deployment.puml`](diagrams/deployment.puml).
Keep initial integrations local where possible: MCP stdio, loopback
rigctld, local modem/client APIs, local store, and outbound HTTPS only for
configured evidence. Remote components must use explicit authentication,
authorization, transport security, and replay handling. Human-facing runtime
controls expose telemetry and the independent emergency stop only. Station
limits and autonomous scope are installed in a signed, versioned profile
before service activation.

Implementation sequence:

1. Domain types, ports, fake adapters, deterministic clock, event model, and
   property-tested policy/state machines (S0 simulator).
2. Inventory/provider adapters and complete radio capability discovery;
   read-only operation, freshness, rank explanations, and shadow mode.
3. Serialized capability-driven CAT control with leases, per-operation
   policy, readback, compensation, extension schemas, and fault behavior;
   candidate preparation is validated as a common use case.
4. Documented Winlink client and modem session adapters; policy-authorized
   automatic session start, autonomous retry/recovery, and always-available
   emergency stop.
5. Qualified PTT driver/arbiter and watchdog under inhibited or
   shielded/conducted hardware test; automated key-up remains disabled until
   measured safety acceptance criteria pass.
6. Bounded automation and empirical ranking improvements only after
   simulation, shadow operation, and supervised station validation.

No single dependency can make the design safe by itself: MCP read-only
annotations, model instructions, Hamlib, client APIs, process separation,
software timers, and predictions are each insufficient as authorization or
physical protection.

## 9. UML diagram index

- [`layered-architecture.puml`](diagrams/layered-architecture.puml) — trust
  boundary, layers, components, and allowed dependency direction.
- [`interfaces-and-domain.puml`](diagrams/interfaces-and-domain.puml) — ports,
  service contracts, and principal domain records.
- [`rank-candidates.puml`](diagrams/rank-candidates.puml) — read-only evidence
  aggregation and deterministic ranking.
- [`prepare-candidate.puml`](diagrams/prepare-candidate.puml) — lease,
  validation, configure/readback, and compensation sequence.
- [`authorized-session.puml`](diagrams/authorized-session.puml) — grants,
  client/modem handoff, permit-based PTT, abort, and audit events.
- [`session-state-machine.puml`](diagrams/session-state-machine.puml) — local
  session lifecycle and non-bypassable safety transitions.
- [`information-model.puml`](diagrams/information-model.puml) — persisted
  inventory, evidence, authorization, session, and event associations.
- [`deployment.puml`](diagrams/deployment.puml) — initial workstation
  deployment and network/device interfaces.

## 10. Open design decisions

Before implementation freeze, resolve: exact supported Winlink client and
documented automation API; modem command/transport evidence; radio-backend
capability and PTT route; hardware independent watchdog/inhibit design;
policy thresholds and station-profile schema; database/event-retention
settings; autonomous service identity and capability restrictions; source-specific freshness and
rate limits; and how ambiguous transfer completion is reconciled. Record
choices as ADRs and keep provisional items visibly marked.
