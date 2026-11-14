# NIAP evaluation-readiness plan

**Objective.** Produce a reproducible evaluation package for one Linkwise
release configuration and obtain a documented decision on whether it can
enter NIAP evaluation. Formal validation requires independent evaluation
and a NIAP determination.

**Proposed starting configuration.** The packaged Linux modem service on
one named OS release and architecture, with SELinux enforcing where
applicable. Confirm the platform and scope with the intended acquirer and
qualified evaluation lab. Record the exact executable, installer, service
identity, command and DATA sockets, configuration and state directories,
audio interfaces, radio/PTT control, and any platform security dependencies.
For mail forwarding, AI management and secure peer transfer, record whether
each is included, disabled or absent. Verify that out-of-scope functions
cannot bypass any security control claimed for the evaluated configuration.

| Phase | Work | Completion evidence |
| --- | --- | --- |
| 1. Confirm scope | Document mission, data sensitivity, acquisition expectations, threats, operational assumptions and proposed Target of Evaluation (TOE). Ask the lab to confirm the applicable Protection Profile revision, modules, interpretations, evaluation activities and platform coverage. | Written profile and scope disposition. If no suitable route is identified, record the gap and disposition before committing to evaluation-specific engineering. |
| 2. Draft the Security Target | Identify product and environment security responsibilities. Draw the trust boundaries and enumerate entry points, privileged actions, secrets and assets. Map each applicable security functional and assurance requirement to its implementation, guidance, and verification method. | Reviewed draft Security Target and a matrix with one traceable entry per requirement and evaluation activity. Record the justification for each selection or exclusion. |
| 3. Implement and verify controls | Prioritize service privilege, socket authorization, sensitive state, bounded parsing, release authenticity and failure containment according to the matrix. Exercise malformed RF and network data, unauthorized local clients, resource exhaustion, and interruption during state changes. Where PTT is in scope, test release on watchdog expiry and loss of control using a hardware-inhibited or conducted bench. | Identified tests run against installed packages, with logs for allowed and denied operations and recovery outcomes. |
| 4. Qualify crypto and host hardening separately | Inventory the selected crypto paths, including secure peer transfer and SMTP/IMAP TLS if in scope. For each FIPS claim, map the exact module build, CMVP certificate, security policy, approved mode and operational environment. Verify applicable host controls with SELinux enforcing, including contexts, authorized accesses, and denial of cross-boundary access. | Per-path cryptographic evidence or a documented scope gap. Host checklist with policy version, installed contexts, positive tests and observed denials. |
| 5. Freeze the release and operator guidance | Record source revision, build inputs, dependency inventory and signing details. Document secure installation, version checks, configuration, operation, updates, recovery, and removal. Run vulnerability scans and record applicability and dispositions. | Signed, versioned release candidate. A second operator can install it from the guide, verify authenticity, reject tampered updates and reproduce the evidence on the candidate platform. |
| 6. Conduct lab readiness review | Present the scope decision, Security Target, traceability matrix, package, operational guidance and test results. Assign owners and acceptance checks for any findings. Agree on change control and evidence updates after submission. | Written lab feedback and a decision to submit, remediate, or revise scope. A readiness decision and an issued validation certificate are separate milestones. |

## Evidence record

For each requirement, capture the requirement and evaluation activity ID,
scope and selection rationale, implementation and guidance references,
artifact SHA-256, platform and configuration, steps and expected outcome,
actual result, evidence location, and reviewer disposition. A requirement
with only developer CI evidence remains pending where independent evaluator
evidence is required. Track deployment authorization separately from NIAP
product validation and CMVP module validation.

## U.S. government references

- [NSA Standards & Certifications](https://www.nsa.gov/about/cybersecurity-collaboration-center/standards-and-certifications/)
- [NSA Protection Profile FAQ](https://www.nsa.gov/portals/75/documents/resources/everyone/csfc/csfc-faqs.pdf)
- [NIST SP 800-160 Vol. 1 Rev. 1, Engineering Trustworthy Secure Systems](https://csrc.nist.gov/pubs/sp/800/160/v1/r1/final)
- [NIST SP 800-218, Secure Software Development Framework](https://csrc.nist.gov/pubs/sp/800/218/final)
- [NIST CMVP validated modules](https://csrc.nist.gov/projects/cryptographic-module-validation-program/validated-modules)
- [NIST SP 800-37 Rev. 2, Risk Management Framework](https://csrc.nist.gov/pubs/sp/800/37/r2/final)

## Execution

See the [candidate scoping package](niap-candidate.md) for the proposed TOE,
draft security objectives, traceability matrix, and installed-host procedure.
