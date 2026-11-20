# Adaptive network: research and implementation evidence

**Document ID:** LINKWISE-RES-NET-001

**Reviewed:** 19 November 2026

**Source baseline:** `24c50964e8f1b727cc01181efab731a71667a9dd`

**Scope:** Primary literature, standards, implementation sources, selected trade reporting, and existing Linkwise artifacts

This review supports the [implementation plan](adaptive-network-implementation-plan.md).
It distinguishes field operation, controlled measurements, simulations,
theoretical results and vendor proposals. Inclusion does not qualify a dependency
or establish global HF performance. External software was inspected, not built
or interoperability-tested in this review. Moving source branches must be pinned
to tested revisions during implementation.

## 1. Findings that determine the design

1. **An HF relay precedent already exists.** Winlink Hybrid combines local durable
   messages, automatic HF forwarding, optional Internet connectivity and
   propagation-assisted route choice. It is a closer deployment precedent than a
   continuously connected Wi-Fi mesh, although public descriptions do not expose
   enough routing internals or measured denominators to reproduce its optimizer.
2. **Bundle delivery and durable responsibility are separate contracts.** BPv7
   supplies a useful standard envelope, but routing, contact selection, storage
   ownership and application processing acknowledgment still require design.
3. **The scarce resource is a mutually usable contact.** Potential radio edges
   cannot be activated independently. Both stations' tuning, half-duplex state,
   interference, setup cost and reverse-control opportunity matter.
4. **Adaptation can hurt.** Probing, advertisements and oscillating route changes
   consume capacity. Stable fallbacks, bounded control traffic and queue-aware
   admission belong in the first network model.
5. **Existing Linkwise evidence is strongest below the network layer.** It proves
   selected decoding and isolated peer workflows, with important failures and
   selection limits. It cannot yet calibrate global route/contact probabilities.

## 2. Standards and reusable implementations

### R01 — Bundle Protocol Version 7

[RFC 9171, January 2022](https://www.rfc-editor.org/rfc/rfc9171.html)
is a standards primary source. It defines bundle encoding and identity, lifetime,
fragmentation/reassembly, status reporting and convergence-layer requirements.
Routing is outside the specified algorithm. Application-agent delivery is not
confirmation that an application processed the content. Section 1.2 distinguishes
BPv7 from BPv6, including moving custody transfer to BIBE.

**Application:** adopt a tested subset and independent-peer interoperability
fixtures. Bound lifetime, fragmentation and reassembly storage. Preserve separate
modem acknowledgment, durable relay acceptance and application receipt events.
The plan's transactional storage and local receipt semantics are recommendations,
not capabilities bestowed by RFC compliance.

### R02 — Bundle Protocol Security

[RFC 9172, January 2022](https://www.rfc-editor.org/rfc/rfc9172.html)
defines BPSec integrity/confidentiality for stored and forwarded bundle blocks.
It does not supply a complete peer-admission, topology-advertisement or key
management system, nor does it guarantee hop-by-hop authentication.

**Application:** separately specify neighbor identity, advertisement replay
protection, endpoint payload protection and storage quotas. Existing Linkwise
TLS/PKI work is reusable input, not an already implemented network trust system.
Select security modes through the commissioned radio-service profile.

### R03 — TCP convergence layer

[RFC 9174, January 2022](https://www.rfc-editor.org/rfc/rfc9174.html)
specifies TCPCLv4 for transporting BPv7 over TCP, including negotiation,
segmentation, acknowledgments and termination. Persistent cross-session partial
transfer recovery must not be assumed from its transfer acknowledgments.

**Application:** use it for suitable IP bearers and independent interop tests.
The Linkwise radio stream needs an explicit convergence adapter and interruption
tests. A TCP socket between a client and local modem is not end-to-end TCP
across the radio link.

### R04 — Custody and endpoint naming need version decisions

The [BIBE working document](https://datatracker.ietf.org/doc/draft-ietf-dtn-bibe/)
is still an Internet-Draft in the reviewed November 2026 material, not a final
RFC. [RFC 9758, May 2025](https://www.rfc-editor.org/rfc/rfc9758.html)
updates the `ipn` URI scheme and encodings.

**Application:** pin the wire/naming profile with the independent peer. Do not
label a new local durable acceptance ACK as standard BPv7 custody or promise
compatibility with old endpoint encodings without testing. The initial simulator
does not depend on BIBE being finalized.

### R05 — dtn7-rs and bp7-rs

The official [dtn7-rs](https://github.com/dtn7/dtn7-rs) and
[bp7-rs](https://github.com/dtn7/bp7-rs) repositories are Rust implementation
primary sources. The daemon offers external routing/convergence integration;
the inspected [Cargo manifest](https://github.com/dtn7/dtn7-rs/blob/master/core/dtn7/Cargo.toml)
includes persistent storage choices and a BPSec feature. The repositories report
MIT/Apache licensing. The README describes ongoing development.

**Application:** compare a daemon with external adapters against library
integration, using the same crash, limit and interoperability tests. Feature
availability is not proof of required durability or target-platform readiness.
Do not silently fork a second store outside the selected agent's lifecycle.

### R06 — NASA/JPL ION

[ION official source](https://github.com/nasa-jpl/ION-DTN),
[documentation](https://ion-dtn.readthedocs.io/en/latest/), and the
[deployment guide](https://github.com/nasa-jpl/ION-DTN/blob/integration/site-docs/docs/deployment-guide.md)
describe persistent storage, contact-graph routing and DTN tooling. The reviewed
guide distinguishes officially supported Linux/Solaris from best-effort macOS
and explicitly identifies automatic inter-regional routing as unimplemented.
The core [license text](https://github.com/nasa-jpl/ION-DTN/blob/integration/license.txt)
has permissive BSD-style terms; do not substitute NASA-wide license assumptions
for the actual selected distribution and third-party audit.

**Application:** independent BP interoperability reference and a candidate
sidecar, with a small configured region first. Its contact plans are not an
HF discovery/rendezvous controller. Global region summaries and autonomous
cross-region forwarding still need Linkwise design and testing.

### R12 — NASA HDTN maintenance state

The official [HDTN repository](https://github.com/nasa/HDTN) states that its owner
archived it on **3 November 2026**. Its ingress/store/router/egress decomposition
and contact-plan handling remain useful references. The
[implementation wiki](https://github.com/nasa/HDTN/wiki/HDTN-Implementation-Features)
contains older feature/roadmap material that should not override newer source.

**Application:** reference or benchmark, not the default actively maintained
dependency. High-rate C++ optimization does not resolve Linkwise's immediate
low-rate contact, rendezvous and station-control problems.

## 3. Deployment experience and routing constraints

### R07 — Winlink Hybrid: the closest HF relay precedent

The operator's [Hybrid Network introduction](https://winlink.org/HybridNetwork)
dates the RMS Relay release to November 2013, describes testing among **43 RMS
stations**, and states SHARES deployed and exercised the system. Gateways store
messages locally, compute the next route toward selected message-pickup stations,
and use VOACAP predictions. Disconnected gateways exchange short service
messages. [RMS Relay documentation](https://winlink.org/content/rms_relay)
describes local storage and radio-only forwarding roles.

**Evidence class:** deployment described by the operator; the introduction is
historical, not a contemporary performance study. It does not establish a
global delivery SLA, current installation count, or publish the full route cost.
Old version recommendations are not installation instructions for this project.

**Application:** forecast-assisted relay routing is a baseline to beat. Keep
message-pickup/destination identity and relay next hop distinct. A surrogate in
the simulator must be labeled as such, not claimed as an exact RMS replica.

### R08 — Isode HF/ALE, HF-RIP and congestion experience

[Not Using ALE for Frequency Selection](https://www.isode.com/whitepaper/not-using-ale-for-frequency-selection/)
(Nordic HF 2022) discusses narrow sounding versus data performance, sounding
overhead, asymmetric links and frequency choice above ALE.
[IP Routing Over HF](https://www.isode.com/whitepaper/ip-routing-over-hf/)
describes implemented Icon-PEP experience and proposed HF-RIP extensions. It warns
that rapid route changes can break proxied TCP sessions. The
[routing sublayer proposal](https://www.isode.com/whitepaper/stanag-5066-routing-sublayer-s5066-ep13/)
distinguishes application relays from lower-layer routing across HF subnets.
[Congestion control for C2](https://www.isode.com/whitepaper/congestion-control-for-c2-over-hf-radio/)
illustrates why obsolete state updates should not consume the queue like files.

**Evidence class:** vendor implementation experience mixed with proposals and
product descriptions; not an independently measured global mesh deployment.

**Application:** charge establishment/sounding and route-switch costs; separate
reliable objects from supersedable state updates. Use the existing modem as a
bearer without claiming STANAG/ALE compatibility for the new overlay.

### R09 — Babel stability and correctness

[RFC 8966, January 2021, especially §3.6](https://www.rfc-editor.org/rfc/rfc8966.html#section-3.6)
explains feasibility constraints, metric-driven oscillations and hysteresis.
The specification also notes periodic-update overhead. Its routing correctness
properties apply under its stated prefix/metric rules, not arbitrary modified
queue costs or intermittently disconnected overlay behavior.

**Application:** borrow sustained-improvement and fallback principles; benchmark
control overhead. Using a Babel-inspired policy does not justify claiming Babel
loop guarantees or assuming continuously connected IP forwarding is sufficient.

### R10 — Collection Tree Protocol

Gnawali et al., [SenSys 2009 original paper](https://sing.stanford.edu/pubs/sensys09-ctp.pdf),
evaluated **12 testbeds, 20–310 nodes**, with **90.5–99.9%** delivery across
configurations. Adaptive beaconing and feedback from data forwarding reduced
beacons and accelerated loop recovery compared with the selected baseline.
The authors explicitly distinguish these testbeds from field deployments.

**Application:** update contact quality from observed outcomes, react rapidly
to evidence of inconsistency and suppress unnecessary control traffic in stable
periods. A collection tree toward sinks does not by itself solve all-to-all
mail/file delivery or prolonged partitions.

### R11 — Roofnet field measurements

Bicket et al., [MobiCom 2005 original paper](https://pdos.csail.mit.edu/papers/roofnet%3Amobicom05/roofnet-mobicom05.pdf),
measured **37 urban nodes**, average **three-hop** routes and **627 kbit/s**
inter-node throughput. Multi-hop operation improved connectivity and throughput
over the considered single-hop alternative. The paper explicitly leaves multiple
concurrent flows, scalability and long-term evolution outside its evaluation.

**Application:** measured transmission cost is more useful than hop count alone.
Do not extrapolate single-flow results to shared-channel global traffic. Add
concurrent loads, relay bottlenecks and route changes to Linkwise evaluation.

### R17 — Industrial SmartMesh: useful evidence discipline

Analog Devices' [qualification report](https://www.analog.com/en/resources/technical-articles/2021/03/29/06/43/verifying-smartmesh-ip-data-reliability-iot.html)
reports **43,792,812 packets sent and 43,792,756 received over 27 days**, with
100 nodes, four-hop depth and one packet per 30 seconds per node: approximately
**99.99987%** delivery. It describes automated churn and extensive pre-release
network exposure. The vendor's
[application notes](https://www.analog.com/media/en/technical-documentation/application-notes/smartmesh_ip_application_notes.pdf)
explain poor connectivity, interference, oversubscription and bottlenecks.

**Evidence class:** vendor-reported local-mesh measurements/qualification,
not an HF guarantee. Link stability and final delivery can differ because retries
consume additional airtime and energy.

**Application:** publish offered-load denominator, duration, topology, failures,
energy and latency together. Reliability alone can conceal inefficient retries
or severe delay.

### R18 — Trade reporting: engineering and deployment boundaries

*Control Global*, [Deploying wireless safely and securely, May 2015](https://www.controlglobal.com/network/wireless/article/11325738/deploying-wireless-safely-and-securely),
includes named operator experience at Valero Wilmington: **30 transmitters and
11 field access points**, using ISA100 for non-critical monitoring.
*Control Engineering*, [Bring IIoT capabilities to refineries and process plants](https://www.controleng.com/bring-iiot-capabilities-to-refineries-and-process-plants/),
discusses WirelessHART density, redundancy and segmentation as installations grow.
ADI's diagnostic material in R17 provides primary technical corroboration for
capacity/interference constraints.

**Evidence class:** trade reporting with operator/vendor accounts, not controlled
comparative routing trials. These cases establish practical usefulness and
engineering limits, not global autonomous HF performance.

**Application:** self-organization cannot manufacture missing coverage or spare
capacity. Test sparse placement, gateway power loss, redundant paths and the
additional burden placed on surviving relays.

### R21 — Operational NASA DTN

NASA's [PACE report, April 2024](https://www.nasa.gov/science-research/earth-science/nasas-near-space-network-enables-pace-climate-mission-to-phone-home/)
describes operational DTN for housekeeping telemetry. The
[current program overview](https://www.nasa.gov/communicating-with-missions/delay-disruption-tolerant-networking/)
reports more than 34 million successfully transmitted bundles and describes
automatic contact-triggered transfer and interruption recovery.

**Evidence class:** operator-reported mission operation. The published success
percentage lacks the complete per-attempt/lifetime/deadline accounting needed
to import it as a Linkwise reliability target. This is not evidence of autonomous
HF topology optimization.

**Application:** persistent asynchronous transfer has operational precedent.
Separate eventual successful delivery from retry cost and deadline compliance.

## 4. Modeling and algorithm research

### R13 — Harris HF simulation with OMNeT++

Weston and Koski, [High Frequency Radio Network Simulation Using OMNeT++, 2015](https://arxiv.org/pdf/1509.03176),
describe a calibrated modem-error abstraction, STANAG 5066 and protocol modeling.
They distinguish short-term Watterson effects from slower channel variation and
note that waveform processing is too expensive for large network simulations.

**Evidence class:** industry-authored simulation methodology, not Linkwise
calibration data or a deployment trial.

**Application:** keep traffic, bearer and channel models distinct. Use measured
session abstractions for global runs and selected waveform tests for calibration.
A generic LOS model or independent packet-erasure coin is insufficient for HF.

### R14 — Analytical ALE channel occupancy

Baynat et al., [On the Design of Automatic Link Establishment in High Frequency Networks, 2017](https://www.jstage.jst.go.jp/article/ijnc/7/2/7_419/_pdf),
construct channel-oriented Markov models and compare them with OMNeT++ behavior.
They examine occupancy, setup success/failure and effects of network load,
channels and call duration. The validation simulation reproduces simplifying
assumptions; it is not independent field validation.

**Application:** setup/discovery cannot be modeled as free. Use analytical
occupancy checks to catch impossible simulated throughput, then measure actual
Linkwise rendezvous and session behavior rather than borrowing ALE timings.

### R15 — Uncertain contact plans

Raverta et al., [Ad Hoc Networks 123, 2021, author-hosted paper](https://www.cs.famaf.unc.edu.ar/~dargenio/sites/default/files/pdf/papers/paper-365.pdf),
study routing between perfectly scheduled and purely opportunistic contacts.
RUCoP, L-RUCoP and CGR-UCoP incorporate uncertainty, replication and failure
reaction; their comparisons are simulation results, not an HF deployment.
The [CCSDS SABR standard](https://ccsds.org/publications/allpubs/entry/3167/)
targets scheduled time-varying connectivity.

**Application:** retain confidence and missed-contact reaction time. Compare
scheduled-contact routing against uncertain-contact policies. Correlated HF
failures and uncertain usable volume require additional modeling beyond an
independent probability of contact existence.

### R16 — Simulator selection

[The ONE](https://akeranen.github.io/the-one/), its
[original paper](https://www.netlab.tkk.fi/~jo/papers/2009-03-simutools-one.pdf),
and [trace-import guidance](https://www.netlab.tkk.fi/tutkimus/dtn/theone/qa.html)
provide DTN routing/buffer and contact-trace capabilities. The
[ns-3 event manual](https://www2.nsnam.org/docs/release/3.45/manual/html/events.html)
describes discrete-event operation. Neither supplies a qualified Linkwise HF
model merely by being selected.

**Application:** Rust DES is the proposed first implementation because the same
policy/state code can run in the daemon and harness. Export simple contact/message
traces for independent ONE comparison. Consider OMNeT++ or ns-3 if detailed
contention/emulation requirements justify a second codebase. Record why shared
code tests and independent model checks serve different purposes.

### R19 — Propagation and modem-channel standards

[ITU-R P.533-14](https://www.itu.int/rec/R-REC-P.533/en) provides HF circuit
performance prediction. [ITU-R F.1487](https://www.itu.int/dms_pubrec/itu-r/rec/f/R-REC-F.1487-0-200005-I%21%21PDF-E.pdf)
addresses HF modem testing with ionospheric channel simulators up to about
12 kHz. The [ITU-R HF implementation](https://github.com/ITU-R-Study-Group-3/ITU-R-HF)
is a candidate planning-input source.

**Application:** P.533/VOACAP outputs are priors. F.1487 cases can qualify selected
channel tests. Neither predicts an exact future contact nor gives a ready-made
global traffic/interference model. Existing synthetic echoes, AWGN and CFO tests
must not be relabeled as F.1487 qualification.

### R20 — Queue-aware constrained link activation

Tassiulas and Ephremides, [Stability properties of constrained queueing systems and scheduling policies for maximum throughput in multihop radio networks](https://drum.lib.umd.edu/items/571fda52-aefb-4497-9a2d-69d8c7c907b9),
provide a theoretical basis for scheduling under simultaneous-transmission
constraints. The underlying queue-information and buffer assumptions differ from
a finite-storage HF deployment with delayed advertisements and expensive setup.

**Application:** max-weight/backpressure is a useful benchmark, not an automatic
production-optimality claim. Include channel conflicts, setup, deadlines and
stale queue information before comparing it with simpler policies.

## 5. Linkwise test-data audit

This audit read source, structured summaries, trial CSVs, protocols, provenance
and lab interpretation. It did not rerun decoders, live reference-modem sessions,
CI workflows, field transmissions or application demos. The machine-readable
[audit manifest](../testing/data/adaptive-network-evidence-audit-2026-11-19.json)
records selected hashes/counts and explicitly identifies unverified evidence.

### E01 — Artifact availability and reproducibility

`tools/soft-metric-study/results/` has **145 versioned files** at the baseline.
The worktree lacks `rust/target/` and `reference/vara/fixtures/`; downloaded
fixtures and build outputs are intentionally ignored. Some local evidence is
available outside this checkout, with locations in committed provenance.

Five original passive-capture WAVs were found and their SHA-256 values checked
against the committed summary: **17,514,012 bytes** in total. The separate
acceptance-retest archive has **81 WAVs / 212,453,532 bytes** present; a complete
hash audit of those files was not performed. Presence is not reproduction.

Checked current acceptance summary/CSV, structured-negative holdout and mixed
interop summary against their archived checksum index, and the level-4 later
CSV against its provenance. Decoder results remain historical results. Portable
reproduction still needs a content-addressed corpus map and source/session
selection metadata, not machine-specific absolute paths alone.

### E02 — Latest passive KiwiSDR observations

Source: [passive README](../../tools/soft-metric-study/results/kiwisdr-passive-2026-11-18/README.md),
[summary](../../tools/soft-metric-study/results/kiwisdr-passive-2026-11-18/passive-summary.json),
[provenance](../../tools/soft-metric-study/results/kiwisdr-passive-2026-11-18/provenance.json).

**10 receiver attempts** produced five recordings: four complete windows and one
short window, **729.772 s** total. Both decoders recovered **12 identical
receptions / eight distinct KISS payloads**, with zero losses or additional
candidate frames. The candidate evaluated 113 failed hypotheses and activated
zero times. Four cross-receiver matches are provisional packet-arrival estimates.

No transmitter-side denominator, complete independent payload oracle, absolute
RF SNR, stock replay or calibrated propagation skill follows. Receiver connection
failures cannot be treated as failed HF paths. This is preservation/regression
evidence on observed traffic, not an estimate of route availability.

### E03 — Level-2 interference/acceptance retest

Sources: [later summary](../../tools/soft-metric-study/results/acceptance-2026-11-18/validation2/validation-summary.json),
[trial CSV](../../tools/soft-metric-study/results/acceptance-2026-11-18/validation2/validation.csv),
[negative holdout](../../tools/soft-metric-study/results/acceptance-2026-11-18/structured-negatives-holdout.json),
and `lab-notes/lab-notes.tex`, section “Prospective later holdout”. This is newer
and more specific than the top-level DSP README's older unresolved-counterexample
summary; do not collapse the chronology.

The later holdout uses **six selected clips / three payloads**. The stricter
candidate retained **276/300** baseline AWGN recoveries and improved injected
CW recovery **8/60 to 60/60**. All 14 natural continuous receptions were retained,
with no additional natural recovery. The CSV has **1,278 method-trial rows**;
these are repeated perturbations, not 1,278 independent RF transmissions.

In 100 structured-negative windows / 1,050 s, the old gate accepted one CRC-valid
body; baseline and stricter candidate accepted zero. The stricter candidate made
1,191 gate attempts; none of the methods produced generic KISS output. Consecutive
turbo-iteration agreement is correlated and is not an independent checksum.
Production is unchanged; impaired mixed-stock sessions activating the new path
and broader independent negatives remain pending.

### E04 — Level-4 tone cancellation

Sources: [study README](../../tools/soft-metric-study/results/cw-level4-2026-11-18/README.md),
[later trials](../../tools/soft-metric-study/results/cw-level4-2026-11-18/level4-later.csv),
[provenance](../../tools/soft-metric-study/results/cw-level4-2026-11-18/provenance.json).

The later set has **three archived stock frames from two successful sessions**.
Cancellation recovered **270/270** injected-CW trials versus **79/270** baseline,
with **63/63** clean/AWGN controls retained. The later CSV has **433 rows**,
including 100 noise/CW negatives; a separate structured panel brings negative
constructions to 300, with no CRC-valid body. Extended negative exposure remains
below the planned qualification target. Stock live preflight was blocked by guest
agent availability. This is a research receive prototype, not session-level
benefit, absolute sensitivity, or a production real-time implementation.

### E05 — CFAR and receiver diversity

`lab-notes/lab-notes.tex`, section covering CFAR analysis, reports later exact
recovery of baseline **60/80**, calibrated normalized **60/80**, cell averaging
**59/80**, median **0/80**, and Z-score **60/80**. False acquisition candidates
differ from false decoded frames. The retained production threshold is normalized
**0.20**. No tracked CFAR JSON/CSV was found and the cited external CFAR archive
was not verified in this review; those values remain report-level evidence.

The [DSP README](../../tools/soft-metric-study/README.md) describes earlier
known-synchronization diversity recovering **20/20 versus 17/20** at 0 dB
recording/added-noise ratio. A later equal-weight combiner lost **16/20** against
the stronger branch at −8 dB; matched event windows still supplied locality.
Neither establishes autonomous global receiver association or a production gain.

### E06 — Peer-transfer and application evidence

[VARA interoperability records](../testing/vara-interoperability.md) describe
four mixed wide-mode role/bandwidth cases meeting three consecutive radio-free
passes at `70cf5ed`, including corruption after the acknowledged 89-byte prefix,
exact **203 + 6 + 203 bytes**, disconnect and a fresh reconnect probe. Current
worktree raw fixtures for those runs are absent. Earlier failures and missed
injections remain relevant; final results are role/build/condition-specific.

The [latest candidate interop summary](../../tools/soft-metric-study/results/acceptance-2026-11-18/interop-summary.json)
records one ordinary BW500 pass per role with **13 + 6 + 13 bytes** and both
disconnect reports. Caller/responder setup times are approximately **10.50/9.61 s**
and transfer times **27.49/24.38 s**. No impairment was injected and candidate
activation was not demonstrated. Audio relay and pacing affect these timings;
they are not universal HF session parameters.

[Mobile/mail validation](rocky-thunderbird-validation.md) includes the final
Gmail Android workflow: TLS submission, two virtual BW500 peers, matching
**1,400-byte attachment**, both disconnects, and downloaded recipient content.
The harness published the received peer copy into the test IMAP inbox. Other
mobile workflows also used a separate one-shot virtual forwarding harness.
These are useful application-boundary fixtures, not a persistent autonomous
multi-hop deployment. Native modem activity outside the virtual path does not
turn that path into physical RF evidence.

### E07 — Existing tests and what CI does not establish

[Runtime tests](../../rust/tests/modem_runtime.rs) exercise explicit virtual
time, bytes accounting, loss/recovery and release evidence with simulated devices.
[BW500 replay tests](../../rust/tests/bw500_replay.rs),
[wide replay](../../rust/tests/wide_level4_replay.rs), and
[native audio](../../rust/tests/native_audio_loopback.rs) contain ignored cases
requiring external fixtures or configured audio devices.

`tools/hf-channel.py` supplies AWGN and frequency shift; it is not a Watterson
implementation. Other GNU Radio bench profiles also require their exact parameters
and calibration to be stated. The CI definitions cover packaging, virtual mail,
secure-transfer checks, channel utilities and Pat in isolated environments;
definition is not evidence that a workflow ran successfully at this review date.

Fresh checks in this review: offline Cargo metadata loaded successfully;
selected artifact hashes/counts and local raw passive WAVs were verified;
documentation links and diff were checked. No compiled Rust test suite or
modem replay was rerun for these documentation-only changes. The checkout lacks
build output and the host had about 969 MiB free at inspection, so this review
did not initiate a large rebuild or modify/delete unrelated caches.

## 6. Data gaps and immediate collection priorities

The [KiwiSDR campaign plan](../../rust/docs/kiwisdr-test-plan.md) still labels
controlled known-payload RF qualification and held-out VOACAP comparison as
prospective. The global network cannot be calibrated by counting only successful
decoded beacons or perturbing a few selected frames many times.

| Missing quantity | Required collection | Use in the implementation plan |
| --- | --- | --- |
| Contact availability | All offered calls with endpoint/channel availability and failures | Separate no-contact from no-service and fit uncertainty |
| Setup and usable volume | Role/profile/size-specific connect, retries, payload, teardown and reverse receipts | Contact scheduler and chunk-size selection |
| Shared-channel efficiency | Occupancy, interference, listener tuning, hidden terminals, simultaneous offered load | Validate medium and rendezvous rather than independent pairwise cables |
| Relay durability | Crash at every commit/ACK/delete boundary, disk-full and corrupt state | Durable acceptance and duplicate/receipt protocol |
| Topology stability | Delayed/lost advertisements, churn, queue pressure and reservation conflicts | Hysteresis, fallback and bounded control rate |
| Global forecasting skill | Prespecified site/date/season holdouts and forecast issue timestamps | Validate priors without future-information leakage |
| Physical station safety | Independent inhibit/watchdog, readback, measured release and fault response | Gate commissioned autonomous RF operation |

The implementation plan deliberately allows deterministic correctness and
synthetic scale work to proceed while these measurements are pending. Those
results must remain labeled by fidelity and cannot qualify real-world routing
efficiency until their owning assumptions are measured.
