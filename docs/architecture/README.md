# Linkwise system design documents

These documents describe the proposed architecture for AI-assisted radio and
Winlink link management. They are design baselines, not descriptions of
implemented functionality. The current Rust crate is still a scaffold.

## Document set

- [System architecture](system-architecture.md) — context, stakeholders,
  viewpoints, components, deployment, data flow, decisions, and traceability.
- [Software design](software-design.md) — layered runtime, component
  responsibilities, typed software ports, transaction flows, session/PTT
  state machines, data model, and deployment design, with PlantUML sources.
- [Requirements and use cases](requirements-and-use-cases.md) — prioritized
  system requirements, actors, use cases, and proposed verification methods.
- [Winlink link inventory and tracking](winlink-link-tracking-design.md) — the
  definitions, data model, counters, MCP tools, refresh strategy, and acceptance
  checks for managing gateway/channel opportunities and actual session history.
- [RF and AI link-management simulation](rf-agent-simulation-design.md) — the
  proposed fidelity ladder, agent/truth isolation, scenarios, baselines,
  metrics, and validation gates for evaluating link-selection decisions.
- [MCP interface design](mcp-interface-design.md) — proposed resources, tools,
  schemas, errors, and control contracts, including policy-gated PTT key-up
  and always-available key-down.
- [Safety, security, and reliability](safety-security-reliability.md) — safe
  states, hazards, invariants, authorization, leases, and failure handling.
- [Architecture whitepaper source](../whitepaper/linkwise-architecture-whitepaper.tex)
  — stakeholder-oriented narrative and system overview in LaTeX.

## Status and change policy

All statements about capabilities not present in the source tree are proposals.
Requirements with provisional timing or quality values are marked `TBD`; they
must be replaced by measured values before they become acceptance criteria.
Architecture decisions are marked `Proposed` until confirmed by implementation
and review. The evidence ledger and modem research status remain in
[`../../lab-notes/lab-notes.tex`](../../lab-notes/lab-notes.tex).

## Template survey

The supplied [`acq-latex-templates` repository](../../reference/acq-latex-templates/)
was cloned at commit `fef160e` and surveyed. Its Software Architecture
Description (SAD), Software Design Description (SDD), and Software Requirements
Specification (SRS) structures informed the viewpoint, component, traceability,
and qualification organization in this document set. The repository's
`dodacqdoc` class and acquisition-specific marking conventions were not reused;
the whitepaper uses a standalone `article` class.
