# RF and AI link-management simulation design

**Document ID:** LINKWISE-DES-SIM-001  
**Version:** 0.1, proposed  
**Status:** Research-based design proposal; simulator not implemented

## 1. Objective and evaluation question

Build a repeatable environment in which the actual Linkwise agent/tool loop
can observe imperfect station and Winlink data, select/prepare one candidate,
and supervise bounded sessions while the evaluator knows the simulated
environment truth. The core question is not merely “did a simulated link
connect?” It is: given the same RF and gateway scenario, did the policy
deliver more useful queued traffic, sooner and with fewer wasted attempts,
without violating radio/PTT constraints?

One transceiver serves one RF channel at a time. Candidate inventory coverage,
attempted sessions, connected sessions, and message transfer are separate
outcomes. The agent must not improve its score simply by claiming more
channels are available or by making more transmit attempts.

## 2. Recommended simulation stack

Use a fidelity ladder; the decision simulator is the first deliverable and
does not require a detailed propagation ray tracer.

| Level | Tool/model | What it answers | Limitation / use |
| --- | --- | --- | --- |
| S0: tool contract | Rust fake radio/modem/client/data adapters; virtual clock and seeded event scheduler | Are MCP schemas, state transitions, leases, PTT permits/watchdogs, retries, and logging correct? | No RF realism; fast exhaustive/fault tests |
| S1: link-decision Monte Carlo | Linkwise Rust discrete-event simulator; configured gateway, station, queue, and latent channel models | Which gateway-selection and retry strategy maximizes delivery utility under noisy/stale observations? | Calibrate transition/success distributions from real trials; does not replace modem tests |
| S1 prior | Local VOACAPL executable and/or standards-derived ITU-R P.533 inputs | Path/hour/month planning priors and a consistent statistical baseline | Statistical circuit prediction, not minute-by-minute Es or a guaranteed session outcome; online calculator scraping is not the simulator |
| S2: baseband modem-in-loop | ITU-R F.1487 HF channel cases; GNU Radio channel blocks or a project-owned compatible channel adapter; WAV/audio pipe to the actual modem | How fading, multipath delay, Doppler, frequency/sample-rate offset, noise, and modem decode/ARQ affect connection and transfer | GNU Radio's general dynamic channel model is useful for impairments, but is not by itself a validated F.1487/Watterson implementation; compare model parameters and test vectors to the Recommendation |
| S3: controlled hardware-in-loop | Real radio/modem/audio chain in a shielded/conducted setup with hardware transmit inhibit or rated attenuators/load | Are CAT/PTT/audio, watchdog, readback, and safe-stop behavior correct on target hardware? | Not a public RF simulation; never connect this test path to an antenna for automated agent tests |

ITU-R P.533-14 is listed in force for predicting HF circuit performance.
ITU-R F.1487 is the in-force recommendation for testing HF modems with
ionospheric channel simulators up to about 12 kHz, and describes a channel
model that can operate at baseband/audio between a modem transmitter and
receiver. GNU Radio's current dynamic-channel block exposes frequency-selective
fading, Doppler, carrier/sample-rate offsets, AWGN, and a random seed. These
are complementary roles, not interchangeable fidelity claims. See the
[ITU-R P.533 status page](https://www.itu.int/rec/R-REC-P.533/en),
[ITU-R F.1487](https://www.itu.int/rec/R-REC-F.1487-0-200005-I/en), and
[GNU Radio dynamic channel model definition](https://github.com/gnuradio/gnuradio/blob/main/gr-channels/grc/channels_dynamic_channel_model.block.yml).

For S1 planning inputs, the repository already documents [VOACAPL](../../reference/propagation-toolbox.md)
as a locally runnable candidate. The [VOACAPL source repository](https://github.com/jawatson/voacapl)
describes a Linux Fortran/GFortran port and command-line invocation. Pin its
version, inputs, output parser, and license review in the simulation manifest.
Use VOACAP/P.533 outputs as priors or baselines; do not use their predicted
monthly reliability as the ground-truth answer to a particular simulated
minute.

Optional future research: physics-based ray tracing (for example PHaRLAP) can
help study specific ionospheric structures, but adds model inputs, licensing,
and scientific validation work. It is not required for S0/S1 and should not
block decision-policy testing.

## 3. S1 environment model

Implement S1 first as a Rust simulation module/crate behind the same domain
interfaces as the live link manager. It has a virtual clock, seeded random
streams, and the following separable actors:

- **Gateway directory:** RMS stations and multiple channels per station;
  channel protocol/band/service eligibility; report intervals, delayed or
  missing heartbeats, maintenance/outage schedules, stale records, and private
  service lists.
- **Latent RF environment (evaluator-only truth):** time-varying path quality
  per gateway/channel; correlated frequency/band openings, local noise and
  interference, random fades, gateway receiver availability, and connection
  and transfer outcomes. Use configurable correlated state transitions (for
  example a hidden-state/Markov process) so adjacent time slots are not
  independent coin flips.
- **Station and client:** one rig lease, frequency/mode/power/readback, PTT
  permit and watchdog, one active session, modem connect/retry state, transfer
  queue, deadlines, and simulated client limitations.
- **Observation layer (agent-visible):** stale RMS snapshots, imperfect path
  estimates, delayed/biased spot reports, noisy RSSI/SNR estimates, unavailable
  sources, and explicit uncertainty. Keep reported status and RF truth as
  different fields.
- **Event ledger:** append-only scenario events and action/result trace with
  scenario ID, seed, virtual timestamp, agent/model/prompt/tool versions,
  policy revision, and correlation IDs.

Scenario controls should cover UTC hour/season, path geometry, candidate
channel inventory, service restrictions, source latency/outages, gateway
availability, interference/noise, fade duration, RF success and throughput
distributions, message sizes/deadlines/priority, and radio/session limits.
An optional sporadic-E overlay can create localized, time-limited path openings
as a labeled *what-if* scenario with independently randomized control periods.
Reentry predictions may be added as an experimental, uncertain observation
stream (prediction issue time, estimated time/window, source, object, and any
available spatial metadata), but do not cause Es or improve channel quality by
default. The evaluator controls the simulated latent conditions; the agent
must not see them. This permits testing whether reentry features contain
incremental information without embedding the hypothesis as truth.

## 4. Agent interface and information isolation

Run the agent against the same read/prepare/session/abort tool contract used by
the product, with simulated adapters substituted underneath. The agent can
read only the observation view: RMS snapshots, source timestamps, modeled
forecasts, station state, queue summary, and its own session outcomes. The
scenario author/evaluator alone can inspect latent RF truth, future outages,
and random outcomes. Never expose those truth values through MCP resources,
tool errors, logs given to the agent, or deterministic seeds that leak future
events.

The harness owns scenario creation and control, not the agent-facing MCP
server. Test-runner operations (not available to an evaluated agent) should
support `load_scenario`, `reset`, `advance_virtual_time`, `inject_fault`,
`run_policy`, `export_trace`, and `inspect_ground_truth`. This separation
allows repeatable tests without adding simulator-admin tools to production
MCP. In wall-clock model evaluations, the harness advances virtual time only
at explicit observation/session boundaries; local safety timers still have
deterministic simulated semantics.

The local scheduler should refresh inventory and record session events
independently of LLM polling. Notify the agent on relevant deltas when the host
supports it; otherwise let it query summary/history tools at decision points.
PTT watchdogs, session timeouts, and abort remain in the deterministic local
policy/session controller, even in simulation.

## 5. Experiments and “best choice” benchmark

Use paired scenario runs: every candidate policy faces the same scenario and
RF random seed, while separate RNG streams isolate modem/channel variation
from policy decisions. Repeat over many seeds and report confidence intervals.
Compare at least:

1. **Current/system baseline:** fixed operator-preferred channel or existing
   client ordering.
2. **Simple policies:** round-robin, strongest/most recent observation, best
   VOACAP/P.533 prior, and empirical per-gateway success-rate ranking.
3. **Agent under test:** model tool use and recovery policy.
4. **Oracle upper bound:** evaluator selects with access to simulated truth
   under the same radio/session/message constraints. This is unattainable by
   the deployed agent and is only an upper-bound/regret reference.
5. **Random eligible choice:** sanity baseline to expose cases where a complex
   agent does not beat a trivial policy.

For reentry-feature evaluation, add paired ablation runs against the same
seeded latent RF scenarios: (a) VOACAP plus operational evidence only, (b) the
same ranker with reentry covariates, and (c) shuffled-time/place covariates as
a negative control. Represent forecast uncertainty and source latency. Use
event/control windows and hold out complete reentry events, paths, and seasons
from tuning. Measure calibration and incremental mission utility with
confidence intervals. A simulator advantage is only a hypothesis-generating
result; live ranking remains shadow-only until field data establishes
repeatable out-of-sample benefit and an operator explicitly enables a bounded
feature. Never allow the experimental feature to alter eligibility, legal
limits, channel-clear checks, session authorization, or PTT policy.

Define a mission-level utility before testing, for example weighted value of
messages accepted for transfer before their deadline minus airtime, retry,
energy, switching, and failure cost. Keep safety constraints hard constraints,
not reward terms: a PTT policy violation invalidates the run regardless of
reward. A suggested metric set is:

- primary: messages/priority-weight delivered to the next confirmed Winlink
  stage by deadline; total time-to-transfer and utility per run;
- link performance: connect probability per attempted eligible channel,
  attempts to first connect, throughput, session duration, failure categories;
- optimizer quality: regret against oracle, rank of the realized best choice,
  success-probability calibration (Brier/log score), and improvement over
  each simple baseline;
- station efficiency/stability: failed attempts, retunes/channel churn, idle
  time, airtime, queue age, and high-value-message latency;
- data/tool quality: stale-data decisions, unsupported-source handling,
  invalid tool calls, duplicate ingestion, and recovery correctness;
- safety: unauthorized key-up count, duration/duty violations, retune during
  transfer, watchdog/abort latency, and release confirmation. All prohibited
  safety events must be zero.

Report counts with denominators, time windows, scenario mix, source coverage,
and confidence intervals. Keep a chronological and scenario-family holdout so
the agent is not evaluated on the same channel/noise/event traces used to tune
its prompts or ranker. Capture the full observation/action trace to explain
failures. If the agent emits a success probability, assess calibration against
the simulator's hidden outcome over a sufficiently large Monte Carlo sample.

## 6. Fidelity and calibration gates

1. **S0 first:** schema/property/state-machine tests for stale data, multi-
   channel gateways, exclusive radio lease, session boundaries, PTT permits,
   clock jumps, duplicate events, and lost adapters.
2. **S1 next:** deterministic seeded scenario suite; compare to simple
   policies and oracle; calibrate success/outage parameters from recorded
   station attempts and preserve uncertainty when data are sparse.
3. **S2 before modem claims:** send real Linkwise transmitter audio through
   repeatable channel realizations into the receiver; verify analog
   impairment, bit/frame error, connect probability, ARQ, and transfer
   behavior. Align tests with F.1487 cases where applicable and version the
   exact model parameters.
4. **S3 after software safety tests:** hardware-specific PTT/readback and
   watchdog checks in a shielded/conducted setup; no external RF path.
5. **Operational validation:** shadow mode/recommendations against real
   observations without autonomous key-up, followed by operator-supervised
   trials. Simulation is evidence about the modeled scenario distribution,
   not proof of real-world optimality.

## 7. Requirements proposed for the SRS

- Every scenario run is reproducible from an immutable manifest and seed.
- The agent uses the production tool schemas against simulated adapters.
- Hidden future/RF truth is inaccessible to the evaluated agent.
- Baselines and oracle are evaluated on paired scenario realizations.
- Failures, successful connections, transferred messages, and delivery
  confirmations remain separate outcomes.
- The simulator exposes no real transmitter and cannot key a physical radio.
- Simulation model, calibration data, and generated results retain provenance
  and are not mislabeled as field measurements.

## 8. References

- [ITU-R P.533-14: Method for the prediction of the performance of HF circuits](https://www.itu.int/rec/R-REC-P.533/en).
- [ITU-R F.1487: Testing HF modems with ionospheric channel simulators](https://www.itu.int/rec/R-REC-F.1487-0-200005-I/en).
- [GNU Radio dynamic channel model block (source definition)](https://github.com/gnuradio/gnuradio/blob/main/gr-channels/grc/channels_dynamic_channel_model.block.yml).
- [VOACAPL Linux port](https://github.com/jawatson/voacapl).
- [KeepTrack API v4](https://api.keeptrack.space/v4/docs) (predicted reentries sourced from Space-Track TIP; observe authentication, rate limits, attribution, and non-commercial license).
- Chiu, Chang, and Liu, [“The Ionospheric Impact of Artificial Space Object Reentry”](https://www.ion.org/publications/abstract.cfm?articleID=20596), Proceedings of the ION 2026 Pacific PNT Meeting (possible association under study; not an operational HF prediction model).
- [PHaRLAP HF ray-tracing toolbox overview](https://www.dst.defence.gov.au/node/1870) (optional future investigation; access/licensing and suitability require review).
