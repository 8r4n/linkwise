# Linkwise assessment acceptance gates

November 13, 2026 Pacific. The project selected the Rocky adaptation.
The assessment target is the RPM-installed receive-only system service on
Rocky 9.8 ARM64. The cryptographic and secure-boot
platform gate remains open. The internal verdict remains **withhold a pass**.

## Draft Security Target decisions

The security problem covers untrusted audio, untrusted local clients,
unauthorized configuration access, network exposure, and deployment of
an altered package. Management is restricted to the station administrator.
The modem operates receive-only on virtual ALSA and reports counters over
IPv4 loopback. OS authentication, package signature verification, storage
encryption and network filtering are environment dependencies. The RPM's
general-purpose CLI and user unit are delivered components requiring a
scope decision. Establish the applicable Protection Profile and obtain
written agreement on the platform and boundary before assigning formal
functional or assurance components.

The project owner designated the remaining 31 failed and 10 unchecked Rocky
scan findings as not applicable to the HF-only production profile. The scan
counts remain the original observations. Independent acceptance of those
dispositions remains open. See the [findings register](niap-rocky-stig-findings.md).

## Evidence matrix

| Gate | Evidence | Disposition |
| --- | --- | --- |
| Scope and platform | Draft boundary and LW-01 through LW-07 matrix in the [evaluator report](niap-evaluator-report.md). | Rocky adaptation selected for project testing. External acceptance is pending. The native DISA result is retained for traceability. |
| Kernel and storage crypto | Running kernel 5.14.0-687.66.1.el9_8.aarch64, `fips_enabled=1`, encrypted mounts and swap. Captured current kernel SHA-256 and package inventory. | No matching evidence for the installed kernel build and Apple silicon UTM under CMVP 5452 or 5113. |
| OpenSSL | `openssl-fips-provider-3.5.8-2.el9_8.aarch64` reports Red Hat Enterprise Linux OpenSSL FIPS Provider `3.5.8-98f4010eda9fa7ed`. | Certificate 5116 describes Rocky9.20250210 on tested Intel hardware and lists no vendor-affirmed environment. Installed identity and operational environment cannot be credited under 5116. |
| Boot integrity | Separate EFI and boot partitions. Guest `mokutil --sb-state` reports Secure Boot unsupported. | Require verifiable supported boot integrity on the accepted platform. |
| STIG | Full final Rocky profile gave 397 pass, 31 fail, 10 unchecked and 38 inapplicable. Applied additional sudo, PAM, login timeout, file permissions and firewall controls. Runtime firewall permits SSH only, with DROP target and forwarding disabled. | Both login audit rules passed after a cold boot and generated directed audit events. GRUB denied an unauthenticated edit and booted normally. Its two automated checks and authorized recovery remain open. Site findings include authenticated remote logging, audit notification delivery, approved DNS/NTP, smart-card enrollment and authorization records. Track each failed and unchecked control in the [Rocky disposition register](niap-rocky-stig-findings.md). |
| Release trust | Bench-signed RPM identity and isolated negative signature checks remain recorded in the evaluator report. | Production signer and a distinct signed release are needed for a genuine upgrade and recovery test. |
| Network isolation | Separate Linux network namespace reached the guest SSH positive control. Status and DATA ports timed out under the final firewall policy. | Isolated network-stack test passed. Independent physical host and physical transmit inhibition remain open. |
| Availability | Replayed 45.21 seconds of deterministic noise while 872 status queries returned ready. Peak sampled cgroup memory was 13,922,304 bytes. One CPU, 128 MiB and 64 tasks enforced. Same daemon PID and no restarts during the run. It then decoded three known BW500 frames. | Pass for the recorded stress case. Longer saturation and physical input-path testing remain open. |
| Clean replay | Provisioning captures the new controls and test scripts. The target disk currently uses several GiB and the host has roughly 2.5 GiB free. | Fresh VM replay pending on the accepted platform with sufficient space. |

The new systemd limits drop-in SHA-256 is
`0ae49752e76a9ae03ef6be1ebd1721c4f8fa6ec67d693f310fc712c4fc0b1e84`.
Local evidence is in `~/Virtual Machines/Linkwise Rocky9 Evidence` in
`gate-*.log` and `scans/rocky-native-hf-only.{xml,html,log}` and `rocky-login-audit.log`.

## Inputs required for final evaluation

- External acceptance of the Rocky platform and applicable profile, plus
  evidence of CMVP coverage for the exact modules and operational environment.
- Authorized site configuration for PKI and enrollment, logging and alerting,
  name and time services, and the approved login notice.
- Production-signed versions and the evaluation host for clean provisioning
  and upgrade tests.

## Government references

- [NIST CMVP certificate 5452](https://csrc.nist.gov/projects/cryptographic-module-validation-program/certificate/5452)
- [NIST CMVP certificate 5113](https://csrc.nist.gov/projects/cryptographic-module-validation-program/certificate/5113)
- [NIST CMVP certificate 5116 security policy](https://csrc.nist.gov/CSRC/media/projects/cryptographic-module-validation-program/documents/security-policies/140sp5116.pdf)
- [NIST CMVP validation scope](https://csrc.nist.gov/projects/cryptographic-module-validation-program/cmvp-flow)
- [NIST Common Criteria Testing](https://www.nist.gov/nvlap/common-criteria-testing-lap)
