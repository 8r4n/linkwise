# Linkwise Rocky 9 evaluator report

**Disposition: Withhold a pass.** This internal assessment applies evaluator
methods to the receive-only candidate and the draft LW-01 through LW-07
objectives. Formal NIAP validation requires an accredited CCTL, an agreed
Security Target and applicable Protection Profile and evaluation activities.
See [NIST's Common Criteria Testing program](https://www.nist.gov/nvlap/common-criteria-testing-lap)
and [NSA evaluation guidance](https://www.nsa.gov/resources/commercial-solutions-for-classified-program/faq/).

## Evaluated configuration

November 14, 2026 UTC (November 13 Pacific). Linkwise 0.1.0-1 aarch64 on
Rocky 9.8, kernel 5.14.0-687.66.1.el9_8.aarch64, Apple silicon UTM.
The assessment covers the installed system service in receive-only mode,
the RPM trust procedure and the associated host controls. Product transmit,
mail and encryption paths received no conformance credit.

| Artifact | SHA-256 |
| --- | --- |
| Bench-signed RPM | `3886d7f57b27bccea0544a4fedb49666c5458ec3b0de29524143fce77b430b29` |
| Installed daemon | `c068a6c0b15342b1096ceedad47063a145f76abd4b201f76bba66c7478fc549e` |
| Station configuration | `25e7b3949029a3d4cfd5c76e324d704a5da7d213a8e5b595c19621dfc0eb042c` |
| System service unit | `55478ecbf9fef045c0882b1cc6211f5f582f261951259750ee720e1f8ace1a61` |

## Verdicts

| Objective | Verdict | Observed evidence and limitation |
| --- | --- | --- |
| LW-01 Release identity | Pass for bench | Hashes and RPM verification matched the recorded install. Release candidate still requires a formal freeze. |
| LW-02 Trusted installation and update | Partial | The Rocky verifier accepted the enrolled bench signature and rejected the same RPM with an empty trust database and a byte-modified copy with trust enrolled. Production-signed upgrade and rollback behavior remain untested. |
| LW-03 Confinement | Pass for tested read boundary | A root-owned, world-readable synthetic station config labeled `shadow_t` was denied to the daemon. Audit event 10528 records `linkwise_modem_t`, denied read and `permissive=0`. Positive ALSA receive passed after restart. |
| LW-04 Receive/network boundary | Partial | 600 local status queries completed, including 300 with injected CONNECT bytes, returning only the expected JSON fields. DATA and tested mail ports refused local connections. Prior guest-interface checks refused both modem ports. UTM NAT limits independent ingress testing and no physical transmit inhibit was exercised. |
| LW-05 Input and availability | Partial | Installed binary rejected truncated JSON, 16 KiB plus one byte, unknown field and duplicate ports. Tested burst completed without a restart. The service stopped cleanly in one second and restarted and decoded three BW500 level-1 frames. Sustained adversarial audio and saturation remain untested. The unit exposes no explicit CPU or memory ceiling. |
| LW-06 Vulnerability analysis | Partial | RustSec matched zero advisories for 234 locked dependencies at database revision `4ef2df8e806a65e2f1ecc14f56f51249062b23d6`. DNF listed no available security updates. Native installed-component advisory coverage and penetration scope remain open. |
| LW-07 Optional features | Partial | Receive status exposed no mail or DATA endpoint during the run. The RPM still ships a general-purpose CLI and a user service. Their impact on the evaluated boundary needs resolution in the Security Target. |

The underlying Rocky STIG scan has 39 failures and 10 unchecked rules.
The official RHEL scan marked 393 rules inapplicable. The evaluator also
observed a missing sendmail executable while audit low-space notification
is configured to use email. Audit logging and the directed denial worked.
A working notification destination must be demonstrated separately.
Kernel FIPS mode and an active OpenSSL provider are recorded. Approved
coverage for the exact installed modules and UTM environment remains
unverified. See the [deployment assessment](niap-rocky9-assessment.md).

## Evidence and replay

The private host evidence directory `~/Virtual Machines/Linkwise Rocky9 Evidence`
contains `evaluator-candidate.log`, `evaluator-signatures.log`,
`evaluator-cargo-audit.json` and `evaluator-os-advisories.log`.
Replay the guest tests using `evaluate-candidate.sh` and
`evaluate-signatures.sh` under `tools/rocky9-vm`, passing the staged
guest evidence directory. The candidate test briefly restarts the service.

Follow-up remediations, platform findings and stress results are recorded in
the [acceptance gates](niap-acceptance-gates.md).
