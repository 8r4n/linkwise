# Linkwise system architecture

**Document ID:** LINKWISE-SAD-001  
**Version:** 0.1, proposed  
**Status:** Architecture proposal; not implemented

## 1. Purpose and scope

This document describes the proposed architecture of Linkwise: an independent
cross-platform Rust modem project and an AI-assisted link-management system for
radio, VARA/Linkwise, and Winlink. It captures system context, stakeholder
concerns, architecture views, design decisions, and traceability to the
requirements in [requirements-and-use-cases.md](requirements-and-use-cases.md).

The system has two separable product concerns:

1. A modem implementation interoperable with an unmodified VARA HF installation.
2. A policy-controlled communications orchestrator that selects, prepares,
   monitors, and learns from Winlink radio links using AI-assisted planning.

The architecture does not assert that a VARA protocol, waveform, Winlink
automation interface, or real-time propagation predictor has already been
implemented. Unknown modem behavior remains gated on lawful reference evidence
and repeatable interoperability tests.

## 2. System context

### 2.1 System boundary

The Linkwise Link Manager (OLM) is the system-of-interest. It runs locally with
the radio and coordinates external services. The AI model is an untrusted
planner with full mediated station-control capability: it can invoke all
locally enabled controls advertised by the Hamlib/radio backend and request
explicit PTT key-up/key-down through MCP. It cannot connect directly to
CAT/PTT/audio devices, open arbitrary sockets, or bypass Linkwise authorization,
leases, audit, or safety supervision.

```text
 AI host/model
      │ MCP tools/resources
      ▼
┌──────────────────────── Linkwise Link Manager ────────────────────────┐
│ MCP server │ scheduler │ policy engine │ state store │ audit log      │
│            │ gateway ranker │ session controller │ PTT arbiter         │
└─────┬──────────────┬──────────────────┬──────────────────┬────────────┘
      │              │                  │                  │
      ▼              ▼                  ▼                  ▼
 Hamlib/rigctld   VARA or Linkwise   Winlink client   Evidence sources
      │              │                  │            VOACAP / RMS /
      ▼              ▼                  ▼            PSK Reporter / GIRO
     Radio       audio/PTT path       Winlink CMS       DX Maps / logs
```

An installation may co-locate all processes or use separate machines. Network
access to rigctld, VARA control ports, or MCP must remain bound to loopback or
an explicitly protected local network unless authenticated transport is
provided.

### 2.2 External systems

| External system | Role | Trust/evidence treatment |
| --- | --- | --- |
| AI host and model | Interpret goals, inspect station state, use CAT/PTT tools, and request link workflows | Untrusted planner; every actuation is schema- and policy-validated and auditable |
| Hamlib `rigctld` | All status and control operations advertised by the configured radio backend | Full CAT capability is exposed through typed Linkwise tools; exact controls vary by radio/backend and remain subject to station policy |
| VARA HF | Existing interoperability reference modem | External process; proprietary behavior is not assumed |
| Linkwise modem | Future native modem implementation | Planned; must implement a verified external interface |
| Winlink client (initial adapter target: Pat where supported) | Message queue and RMS session orchestration | API/client-specific; no universal control API assumed |
| Winlink RMS directory | Gateway/channel and reporting-status source | Time-sensitive inventory, not proof of current RF reachability |
| VOACAP | Statistical path-planning evidence | Monthly/statistical prediction, not an event forecast |
| PSK Reporter / DX Maps | Reported digital activity and spots | Observational, coverage-biased evidence |
| GIRO / DIDBase | Ionosonde data and derived ionospheric characteristics | Local/remote measurements; spatial and temporal limits apply |
| KeepTrack | Satellite-object/reentry timing metadata | Separate event covariate, not propagation evidence |

Hamlib's network-control documentation describes rigctld as a TCP text-command
service; its default port is commonly 4532. Deployment must follow the actual
installed version and radio capabilities, not hard-code assumptions about all
rigs [1].

## 3. Stakeholders and concerns

| Stakeholder | Primary concerns | Framed by |
| --- | --- | --- |
| Station owner/operator | Useful message delivery, understandable decisions, emergency stop, pre-deployment profile commissioning | Operational, safety |
| Station maintainer | Stable radio configuration, recovery, diagnostics | Deployment, reliability |
| Modem developer | Proven interoperability, reproducible signal fixtures | Development, verification |
| Winlink user/sysop | Correct gateway/channel use and client compatibility | Operational, interface |
| AI host integrator | Typed discoverable tools, bounded actions, clear errors | MCP/API |
| Safety/compliance reviewer | Prevent unintended/unauthorized transmission; auditability | Safety, security |
| Project owner | Incremental delivery, portable implementation, evidence-based scope | Roadmap, decision log |

## 4. Architecture viewpoints and views

### 4.1 Functional/logical view

The implementation-oriented design—including typed software ports,
application flows, session/PTT state machines, persistence, and PlantUML
sequence and deployment views—is maintained in the
[Software Design Description](software-design.md).

The system is divided into five trust-relevant layers:

1. **AI planning:** chooses objectives and proposes gateway or recovery actions.
2. **Orchestration:** owns the session state machine, schedules observations,
   ranks gateway candidates, and coordinates adapters.
3. **Policy/safety:** deterministic authorization, limits, lease ownership,
   channel-state gates, watchdogs, and fail-safe transitions.
4. **Adapters:** Hamlib, VARA/Linkwise, Winlink-client, propagation-source,
   persistence, and qualified audio/PTT interfaces, called through the PTT
   arbiter for all keying operations.
5. **Hardware/external services:** radio, gateway network, data sources.

The policy layer is not an LLM prompt. It is ordinary code with explicit,
versioned configuration and testable decisions. All actuator operations pass
through it.

### 4.2 Development view

Proposed Rust workspace boundaries (create when implementation needs them):

| Crate/module | Responsibility | Must not own |
| --- | --- | --- |
| `linkwise-core` | Domain types, state machine, scoring contracts | Device I/O or model prompts |
| `linkwise-hamlib` | rigctld client and capability normalization | Winlink or propagation policy |
| `linkwise-ptt` | PTT arbitration, qualified backend drivers, key-up budget and watchdog | Unscoped key-up or policy authorization |
| `linkwise-modem` | VARA-compatible modem interfaces and future DSP | Radio CAT transport |
| `linkwise-winlink` | Client adapter, channel/session/message operations | Direct PTT or arbitrary rig control |
| `linkwise-winlink-inventory` | Source sync, normalized channel inventory, snapshot counts and freshness | Inferring RF reachability from directory status |
| `linkwise-link-history` | Idempotent session/transfer event ingestion and KPI aggregation | Treating CMS acceptance as recipient delivery |
| `linkwise-propagation` | Gateway inventory, forecast/observation normalization | Transmission authorization |
| `linkwise-policy` | Frequency/power/session rules, interlocks, authorization | AI inference |
| `linkwise-mcp` | MCP resources/tools mapped to use cases | Raw shell or direct hardware bypass |
| `linkwise-store` | History, provenance, event/audit persistence | Mutable control authority |
| `linkwise-sim` (development/test only) | Virtual-time station, gateway, queue, and latent RF scenarios; deterministic replay and policy evaluation | Production control, real PTT, agent access to evaluator truth |

These names are design placeholders; the existing `rust/` crate remains a
single scaffold until dependencies and interfaces are proven.

The simulation harness substitutes adapters beneath the same domain and tool
contracts used by the live link manager. It separates agent-visible evidence
from evaluator-only latent channel state and future events. Simulator control
and ground-truth inspection are test-runner interfaces only; they must not be
registered as production MCP tools. Its virtual clock drives deterministic
session, lease, and watchdog semantics. The simulation fidelity ladder and
acceptance metrics are specified in
[rf-agent-simulation-design.md](rf-agent-simulation-design.md). No simulation
profile may reach a physical transmitter; hardware-in-loop qualification is a
separate inhibited, shielded/conducted setup.

### 4.3 Deployment/physical view

Minimum deployment uses Docker Compose to manage Linkwise services. The
MCP/application service and Hamlib/`rigctld` service communicate over a private
Compose network; only the MCP transport is exposed to its configured local
host. The simulated backend is used in development and cannot reach physical
radio devices. A radio-connected profile grants device access only to the
Hamlib service. Host device pass-through and emergency-stop qualification
remain platform-specific and must be established before radio deployment.

Minimum deployed configuration:

- One autonomous station workstation running OLM, AI host, radio control, modem, and
  Winlink client.
- A radio connected by CAT/USB and an audio/PTT interface.
- Network connectivity for channel metadata and optional propagation sources.
- A physical or hardware-level emergency transmit inhibit accessible to the
  operator.

If the model, internet connection, propagation data, or Winlink service becomes
unavailable, the control stack completes only bounded safe work, then
continues with a qualified deterministic fallback or remains receive-only.
Loss of cloud AI must not block the independent emergency stop. No manual
runtime radio-control mode is exposed; station commissioning/profile changes
are performed offline before activating autonomous service.

### 4.4 Information/data view

Canonical records are timestamped in UTC and carry source, retrieval time,
validity interval, provenance, and confidence/quality metadata. Core entities:

- `RadioSnapshot`: actual reported frequency, mode, power, PTT, signal and
  capability/error state.
- `Gateway`: stable RMS station identity, callsign, location, service code,
  source identifiers, and last report timestamp.
- `GatewayChannel`: callsign, coordinates/grid, frequency, mode, service code,
  status/last report, and source version/time. Count separately from the parent
  gateway identity, retaining source-report time and local ingest time.
- `InventorySnapshot`: source, query filters, unique gateway/channel counts,
  retrieval/source timestamps, warnings, schema/version, and content hash.
- `CollectionRun`: provider and requested scope, start/end times, status,
  coverage, warnings/errors, and the resulting immutable source-record IDs.
- `SourceRecord` / `ForecastRun`: typed, source-attributed observation or
  forecast values with units, location/path, frequency, observed/valid/retrieved
  times, evidence class, quality, uncertainty, source revision, and payload
  reference/hash where retention permits. Forecast runs retain model/version
  and lead-time metadata; later corrections append revisions.
- `StationProfileRevision`: immutable antenna, station location, equipment,
  and operating assumptions used by a specific analysis.
- `AnalysisSnapshot` / `AnalysisInput`: versioned features, estimates,
  uncertainty, coverage, and missing/stale-input reasons. Each input row
  references exactly one immutable target and records its role; the snapshot
  pins algorithm/config versions and the exact input set.
- `HistoricalOutcomeAggregate`: versioned, time-bounded and dimensioned
  aggregate derived from measured attempts/events, including sample counts,
  denominators, coverage, and uncertainty.
- `CandidateLink` / `RankingEntry`: candidate channel and its eligibility,
  score components, rank, exclusions, and explanation tied to one analysis and
  ranking version.
- `Decision` / `PolicyEvaluation`: explicit select, wait, or no-route result;
  selection is optional and policy evaluation records constraints, reasons,
  revision, and authorization reference.
- `SessionAttempt` / `SessionEvent`: one concrete attempt per session try,
  linked to its decision and candidate, with ordered lifecycle events and
  distinct gateway contact, modem connection, client acceptance, and
  recipient-delivery outcomes when observable. Retries create new attempts.
- `TransferEvent` / `QueueSnapshot`: client-observed transfer stage and
  aggregate queue counts, when supported; exclude message bodies and recipient
  data from link-optimization telemetry.
- `PolicyDecision`: request, evaluated constraints, allow/deny, reasons,
  policy/config revision, expiry, and authorization reference.
- `PttTransition`: requested and observed key state, initiator, active session,
  permit reference, driver, timestamps, budget consumption, and release result.

Raw personal message contents are not needed for link optimization. Store
message identifiers and transfer metadata by default; payload retention must be
opt-in and separately protected.

### 4.5 Security/safety view

Trust boundaries exist between model host and OLM; OLM and radio control;
external web/data sources and the local evidence store; and the digital audio
chain and RF transmitter. The model may produce malformed, stale, or
adversarial requests. External gateway and propagation information may be
stale or wrong. The policy gate validates every side effect and uses locally
configured operating constraints. See
[safety-security-reliability.md](safety-security-reliability.md).

### 4.6 Operational control loop

```text
Observe ─► validate freshness ─► build candidate set ─► rank
   ▲                                                  │
   │                                                  ▼
learn from result ◄─ log result ◄─ monitor session ◄─ prepare
                                                     │
                                             policy authorization
                                                     │
                                             bounded session only
```

The one-radio case can service only one RF channel at a time. “Maximize links”
means maximize the available and likely-to-succeed gateway choices over time,
not maintain simultaneous HF circuits on one transceiver. Concurrent circuits
require independent RF chains or receivers and separate resource allocation.

## 5. Gateway selection and optimization

For each candidate gateway (g), the ranker estimates an expected utility:

\[
U(g,t)=P_g(\text{message delivered within budget}\mid E_t)\,V_g
        - C_g^{\mathrm{time}}-C_g^{\mathrm{switch}}-C_g^{\mathrm{risk}}.
\]

Here (E_t) is the timestamped evidence set, and (V_g) is the user-defined
value of the route. Evidence inputs may include RMS availability, path-specific
VOACAP REL/SNR, current third-party reception reports, local measured noise,
antenna bearing, modem history, and session outcomes. They are not independent
truths and must not be blended without confidence, freshness, and provenance.

VOACAP reliability is statistical for the modeled month/hour, not a guarantee
of a specific session [2]. PSK Reporter/DX Maps indicate reported activity,
not guaranteed VARA decode or gateway service. GIRO ionograms characterize
ionosphere near a sounder, not the entire end-to-end path. KeepTrack reentry
times remain an experimental covariate and never directly raise transmission
authorization.

To prevent configuration oscillation, OLM applies minimum dwell time,
hysteresis, switch costs, retry limits, cooldown after failure, and “never
retune during active PTT/transfer” invariants. During a session the chosen
radio configuration is locked except for emergency abort/recovery.

## 6. Proposed architecture decisions

| ID | Decision | Status | Rationale / consequence |
| --- | --- | --- | --- |
| ADR-001 | Use MCP as an AI-facing tool boundary; do not add an LLM dependency to core | Proposed | Allows multiple MCP clients and keeps control policy testable |
| ADR-002 | Keep Hamlib/rigctld as the radio adapter boundary | Proposed | Reuse multi-vendor radio abstraction; capability differs by backend |
| ADR-003 | Separate modem, Winlink client, and rig control adapters | Proposed | They have distinct lifecycle and failure modes |
| ADR-004 | Require deterministic policy gate for all side effects | Proposed | Model output cannot override limits or authorization |
| ADR-005 | First optimize gateway portfolio for one radio, one active session | Proposed | Honest resource model; multi-radio scheduling is a later extension |
| ADR-006 | Keep evidence provenance and confidence with every ranking feature | Proposed | Supports diagnosis and prevents stale/model data being treated as fact |
| ADR-007 | Do not adopt DoW acquisition LaTeX class | Accepted | Its markings, title page, and acquisition workflows are inapplicable |
| ADR-008 | Persist a local, source-stamped gateway/channel inventory and session/transfer event history | Proposed | The AI needs auditable counts and outcomes; model conversation memory is not a reliable system of record |

## 7. Cross-view consistency

| Logical component | Deployment allocation | Requirement family |
| --- | --- | --- |
| MCP interface | Local workstation process; loopback by default | API and isolation |
| Policy engine | Same host as session controller; mandatory in all modes | Safety and authorization |
| Hamlib adapter | OLM process connected to local rigctld | Radio state/configuration |
| Modem adapter | Local process or explicitly trusted host | Modem state and session control |
| Winlink adapter | Supported Winlink client process/API | Gateway sessions/messages |
| Winlink inventory/history service | Local OLM process; source-specific refresh policy | Labeled inventory counts and attempt/transfer metrics |
| Evidence collector | OLM plus network-accessed data services | Freshness/provenance |
| Audit store | Local persistent store | Reconstructable decisions |
| Simulation harness (test only) | Developer/test runner; no production deployment | Paired, reproducible policy evaluation |

## 8. Open issues

1. Confirm a documented and maintainable automation API for each intended
   Winlink client. The initial platform strategy may use Pat where supported;
   do not assume Winlink Express UI automation is a stable interface.
2. Characterize rigctld behavior and capability discovery for target radios,
   especially PTT and power reporting.
3. Specify which VARA TCP commands are documented and supported by the
   installed reference version.
4. Define station configuration: licensed bands/modes, legal limits, antenna
   profile, audio path, maximum power, and operator authorization policy.
5. Define how live RF occupancy is assessed for each band/channel without
   assuming every radio supplies a reliable busy/channel-clear signal.
6. Establish data retention, privacy, and external service terms for gateway,
   propagation, and session data.
7. Select a documented Winlink client/listing integration and confirm which
   session and message-transfer outcomes it can expose.

## 9. Template lineage

Viewpoint, component, correspondence, and decision-log organization was
informed by the cloned `acq-latex-templates` SAD and SDD templates. Those
templates explicitly distinguish outline guidance from actual system content;
Linkwise retains that distinction and does not claim conformance to the DoW
acquisition standards or class.
