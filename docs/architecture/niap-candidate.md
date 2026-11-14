# Linkwise NIAP candidate scoping package

**Disposition.** Internal engineering candidate. Protection Profile applicability
and evaluation entry are pending a written Common Criteria Testing Laboratory
(CCTL) decision. The application software category is a question for the lab,
given the native audio parser and specialized radio function. No profile,
revision, module, conformance claim or final TOE boundary has been selected.

## Draft Security Target scope

The proposed Target of Evaluation (TOE) is the receive diagnostics mode of
the RPM-installed Linkwise modem 0.1.0 system service on Fedora 45 ARM64,
starting from source revision `a2d01b73a88c0968995b767001684ad0f533fddc`.
The version number alone is insufficient to identify the candidate. Record
the RPM hash and installed binary digest, Fedora image identity, exact kernel
and SELinux policy version, unit configuration, and station configuration
hash when freezing the candidate. Fedora is the engineering bench platform.
The acquirer and lab must agree on the deployment OS and host baseline.

The RPM bundles both executables, a system unit, a user unit and SELinux
policy. Scope the running modem daemon, its system unit, executable and
configuration labels, and the installation and update procedures. Identify
the companion CLI and user unit as delivered components whose impact on
security claims needs lab review. The platform supplies systemd, SELinux,
ALSA, package signature verification and administrator identity.

~~~text
Untrusted RF/audio -> host ALSA -> confined receive daemon -> IPv4 loopback JSON status
                                        ^
Station administrator -> root-owned station.json and system unit
                                        ^
Release signer -> signed RPM -> verified package manager installation
~~~

Set the default command port 8300 as the loopback status endpoint. Omit
`bandwidth` and the optional external BW500 profile. TCP 8301 remains closed.
The status endpoint is unauthenticated and can be read by local processes.
Treat counters and readiness as non-sensitive in this configuration. If
the deployment requires per-user status access or confidential diagnostics,
add an authenticated or OS-authorized interface before making that claim.

The configuration uses isolated audio input and has no connected transmit
path. Two-way operation, radio keying, AI management, mail forwarding,
and user-unit operation are outside the proposed runtime. Do not enable
them in the candidate. The standard package build uses `audio-device` and
omits `secure-transfer` and `mail-tls`. The CLI still has mail commands,
including plaintext loopback endpoints when explicitly started. Verify
their absence from the deployment process and listener inventory. Claim no
product cryptographic function for this receive configuration. Treat the
OS package signature verifier as an environment dependency and assess any
future product crypto path separately with its exact CMVP module and mode.

## Threats, objectives, and evidence

These identifiers are local engineering requirements. Replace or map them
to the exact Protection Profile functional and assurance requirements and
evaluation activities after the scope decision.

| ID | Threat and engineering objective | Verification and initial disposition |
| --- | --- | --- |
| LW-01 | Wrong release or altered installation. Identify every executable, build input, package, and runtime profile. | Record commit, RPM and installed digests, toolchain, dependencies and OS baseline. Match package and daemon on host. Developer evidence only until exact candidate is frozen. |
| LW-02 | Untrusted update. Use the enrolled signer and package signature verification. | On a disposable host, install and update a valid signed candidate. Reject a modified package and an unrelated signing key. Production signer and signed upgrade tests pending. |
| LW-03 | Local privilege or configuration modification. Restrict daemon identity and audio access, protect configuration, and enforce the expected SELinux transition. | Run the installed-host verifier, then exercise a permitted ALSA read and a denied cross-boundary read or write with audit evidence. SELinux compile and package smoke do not exercise enforcement. |
| LW-04 | Unexpected network or transmit exposure. Restrict the daemon to loopback status and baseline audio input. | Verify ready status, exact listening address and port, closed DATA socket, and isolated audio route. Exercise external reachability from a second host. The local status socket intentionally allows local reads. |
| LW-05 | Malformed input or resource exhaustion. Reject invalid configuration and contain bad audio without losing service control. | Test oversized and malformed configuration, malformed audio, prolonged input, and clean termination. Record policy denials, recovery, and resource measurements on the installed service. |
| LW-06 | Untracked vulnerabilities. Review dependencies and platform exposure per release. | Capture dated advisory data, dependency inventory, investigation and patch dispositions. Cargo audit scope is the Rust lockfile. OS packages, native libraries and application behavior require separate assessment. |
| LW-07 | Scope bypass through delivered optional tools. Confirm feature flags, listen sockets and default service state. | Inspect the RPM, binary feature configuration and runtime inventory. Attempt access to excluded services. Confirm shipped client binaries and user unit with the CCTL. |

## Administrator evidence run

Use a disposable Fedora 45 ARM64 VM with a functioning ALSA loopback.
Apply the intended host baseline first. Record the VM image, host package
versions and SELinux mode. Archive the candidate RPM, its digest and the
build log. Use the [Fedora installation guide](../install/fedora.md), then
install a root-owned station configuration under `/etc/linkwise` with the
isolated capture and output device names, schema version 1 and a valid
fixture callsign. Omit bandwidth and use default ports. Restore the
configuration label and start the system service. Run the read-only checks:

~~~sh
sudo bash tools/verify-niap-linux.sh /path/to/candidate.rpm
~~~

The verifier checks package digests and does not make a signing claim.

After the first pass, run `tools/test-station-service.py` with a known
level-2 WAV and isolated playback device. Record the frame counter
increment. Repeat after a controlled stop and restart. Collect unit logs
and relevant AVC records. Record the negative test procedure and show that
an unauthorized action was actually attempted and denied. Absence of AVC
records during the positive test is insufficient evidence for denial.

For lab intake, send the scope and feature matrix, representative RPM,
guidance, build and test evidence, and open findings. Ask the CCTL to
confirm product eligibility, applicable profile and evaluation activities,
platform acceptability, and treatment of the delivered CLI and user unit.
Record the lab's written disposition before committing to evaluation scope.

## Results recorded on 2026-11-13

- Built and smoke-tested `linkwise-modem-0.1.0-1.aarch64.rpm` from the
  recorded source in a Fedora 45 ARM64 container with Rust 1.100.0,
  RPM 6.1.0 and SELinux policy development files 45.24-1. The package
  includes the compiled policy module and port labels. The install,
  CLI checks, simulator and uninstall passed. The installed executable
  also accepted the valid configuration and rejected unknown fields,
  duplicate ports, invalid bandwidth, oversize input and malformed JSON.
- Unsigned RPM SHA-256:
  `b7041c987c152a425c9d28bd1b7ae4798693ac05a94d09db4d7108f4c865e5f4`.
  Daemon SHA-256:
  `86e708ff5983a559ba981ad5bb636ef1ff7a8080091e3d764ffe5d51f9bd607f`.
- An isolated RPM key database accepted the copy signed with a disposable
  bench key, rejected the signature with no trusted key, and rejected the
  modified package with the bench key enrolled. The bench signature
  establishes the verification test only. Production signing and a
  signed package-manager upgrade remain pending.
- All 82 audio-enabled library tests and the daemon shutdown-deadline test
  passed. Cargo audit exited successfully against advisory database
  revision `4ef2df8e806a65e2f1ecc14f56f51249062b23d6`, scanning 234 locked
  dependencies. The license audit passed for all six targets. This
  establishes the scan outcome for those inputs.
- The container could compile the policy but lacked SELinux enforcement
  and a running systemd service manager. The Fedora guest agent returned
  access denials when querying enforcement, the RPM database and service
  state. Installed SELinux transition, denial, lifecycle and external
  reachability evidence is pending authorized guest access.
- No CCTL has confirmed a Protection Profile or accepted the candidate.
  This package is sufficient for an intake discussion, with evaluation
  scope, production release signature, and deployment evidence open.

The separate [Rocky 9 assessment](niap-rocky9-assessment.md) tracks the
requested alternative deployment and related evidence.

## Government references

- [NSA Standards and Certifications](https://www.nsa.gov/about/cybersecurity-collaboration-center/standards-and-certifications/)
- [NIST Common Criteria Testing Laboratory Accreditation Program](https://www.nist.gov/nvlap/common-criteria-testing-lap)
- [NIST CMVP Validated Modules](https://csrc.nist.gov/projects/cryptographic-module-validation-program/validated-modules)
- [NIAP engineering plan](niap-evaluation-readiness.md)
