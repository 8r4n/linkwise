# Rocky Linux 9 ARM64 Linkwise assessment

This is developer evidence for the proposed receive-only Linkwise target.
The [candidate scoping package](niap-candidate.md) defines the baseline
requirements. A separate Rocky guest tests whether the deployment
configuration can replace the Fedora engineering candidate. Platform acceptance
and the Protection Profile remain CCTL decisions.

## Build and baseline

- Guest: `Linkwise Rocky 9 ARM64`, UUID `7E30F3CE-F295-4757-B175-AEE8E86A71B4`.
  UTM QEMU/HVF on Apple silicon, with a distinct 32 GiB sparse virtual disk.
- Rocky 9.8 aarch64 boot image SHA-256
  `c40dd3accd0fb08a9848efcddfb29d0b6ba2a43e58a0d54b711e603c28fe9938`.
  Image bytes matched the publisher's checksum. Installation uses the Rocky
  9 BaseOS and AppStream repositories.
- Installer partition layout places root, home, temporary files, var, logs,
  audit logs, and swap inside a LUKS2-backed LVM volume group. The on-disk
  header shows PBKDF2 and AES-XTS. The EFI system partition and boot partition
  are outside LUKS. Verify the mounted layout after boot.
- Linkwise revision `a2d01b73a88c0968995b767001684ad0f533fddc` built for ARM64
  on Rocky with Rust 1.100.0 and passed the package lifecycle smoke test in a
  container. Unsigned RPM SHA-256
  `f1d321eeedfa3511145f9d27cafd167ac4b0f8b4e3e5c6687abe0a27f054d29c`.
  A separate bench-signed copy passed signature verification. The verifier
  rejected the copy with the signing key absent and rejected a modified copy
  with the key enrolled. Bench-signed RPM SHA-256
  `3886d7f57b27bccea0544a4fedb49666c5458ec3b0de29524143fce77b430b29`.

## Assessment boundaries

| Claim | Required evidence |
| --- | --- |
| Updated guest | Successful refreshed DNF upgrade, running kernel version after reboot, and refreshed update check with exit status and timestamp. |
| Encrypted storage | Boot-time unlock, mounted device tree and swap mapping, and verification that every data volume descends from the LUKS mapper. |
| FIPS configuration | Booted kernel FIPS flag, system crypto policy, provider status and exact installed RPM versions. |
| FIPS 140-3 validation coverage | Applicable CMVP certificates, approved-mode configuration, module package identity, and evidence for this operational environment under the security policies and CMVP rules. |
| RHEL 9 STIG controls | DISA RHEL 9 V2R10 manual benchmark and SCAP results, including skipped and inapplicable rules. The DISA data stream identifies RHEL as its platform. Any applicability override or Rocky-specific translation must be identified. Close manual checks and record residual findings. |
| Linkwise installation | Verify the bench signer at installation, run the daemon under SELinux enforcing, measure process and file contexts, verify loopback-only status and absent DATA socket, and exercise an isolated audio fixture. |
| Common Criteria evaluation | Document exact target and release, map applicable requirements to results, and obtain written scope and independent assessment from a qualified lab. |

The V2R10 manual STIG is dated 2026-09-29 and contains 445 rules. Local
evidence includes the original DISA manual, SCAP and Ansible archives and
their hashes. FIPS configuration, CMVP coverage, STIG assessment, and NIAP
validation each require their own evidence.

## Government sources

- [DISA RHEL 9 V2R10 manual STIG](https://dl.dod.cyber.mil/wp-content/uploads/stigs/zip/U_RHEL_9_V2R10_STIG.zip)
- [DISA RHEL 9 V2R10 SCAP benchmark](https://dl.dod.cyber.mil/wp-content/uploads/stigs/zip/U_RHEL_9_V2R10_STIG_SCAP_1-3_Benchmark.zip)
- [NIST Rocky Linux 9 Kernel Cryptographic API certificate 5452](https://csrc.nist.gov/projects/cryptographic-module-validation-program/certificate/5452)
- [NIST Rocky Linux 9 OpenSSL FIPS Provider security policy](https://csrc.nist.gov/CSRC/media/projects/cryptographic-module-validation-program/documents/security-policies/140sp5116.pdf)
- [NSA Standards and Certifications](https://www.nsa.gov/about/cybersecurity-collaboration-center/standards-and-certifications/)

## Verified results

Collected November 13, 2026 Pacific time. After the separate boot filesystem
UUID was added to GRUB, the VM cold-booted with disk unlock and restored
Linkwise automatically. The Kickstart now records that repair. The running
kernel was `5.14.0-687.66.1.el9_8.aarch64`, and a refreshed DNF update check
returned 0. The kernel FIPS flag was 1, the system policy was `FIPS`, the
OpenSSL 3.5.8 FIPS provider was active, and SELinux was enforcing. Seven
mounted data filesystems and swap were individually verified to descend from
LUKS2. EFI and boot are outside the guest LUKS boundary.

The assessed guest has 407 RPMs after removing Bluetooth, the desktop disk
manager, the unused Kerberos credential cache service and crash-dump tools.
The toolchain runs outside the guest. It retains the system security services,
OpenSCAP, and ALSA test tools for this assessment. The only regular interactive
account is `lwadmin`.

| Benchmark | Pass | Fail | Not checked | Not applicable |
| --- | ---: | ---: | ---: | ---: |
| DISA RHEL 9 V2R10 SCAP, final | 0 | 0 | 0 | 393 |
| Rocky Security Guide STIG, initial | 180 | 249 | 10 | 38 |
| Rocky Security Guide STIG, final | 390 | 39 | 10 | 38 |

The DISA benchmark uses RHEL platform checks and provides no Rocky pass
evidence. The full Rocky STIG profile was scanned without tailoring or
applicability overrides. Installed Security Guide was
`scap-security-guide-0.1.82-2.el9_8.rocky.1.1`. Applied controls include
restrictive mounts, network parameters, local RPM signing checks, SSH and PAM
restrictions, audit rules, AIDE, USBGuard, and application control.

Remaining findings include RHEL vendor, subscription and crypto sub-policy
checks, local controls such as bootloader authentication and session policy,
and site requirements for smart card enrollment, approved time and DNS
sources, remote audit, notification delivery and network authorization.
Full STIG compliance is open. Mapping exact installed kernel and provider
builds and the Apple silicon UTM environment to applicable NIST CMVP security
policies is also open.

### Common Criteria developer evidence

| Objective | Observed result | Open evidence |
| --- | --- | --- |
| LW-01 Release identity | Verified RPM digest, installed integrity and running daemon bytes. | Freeze the candidate with a qualified lab. |
| LW-02 Trusted installation | Signature-checked installation passed. Separate fixtures rejected a modified RPM and an unenrolled signer. | Production-signed upgrade. |
| LW-03 Local privilege and SELinux | Dynamic nonroot identity, NoNewPrivileges, `linkwise_modem_t` and expected file and port labels verified. The installed daemon decoded audio with SELinux enforcing. | Directed denial with its audit event. |
| LW-04 Network and receive scope | Status served on loopback port 8300. Ports 8300 and 8301 refused connections to the guest's non-loopback IP. The isolated BW500 level-1 fixture increased the decoded-frame counter from 3 to 6. | Second-host probe and directed transmit-denial test. |
| LW-05 Input and availability | Package smoke and developer tests passed. The guest service restarted and decoded the fixture after hardening. | Installed malformed-input and sustained resource tests. |
| LW-06 Vulnerability tracking | Captured installed RPM inventory and confirmed no pending updates from enabled repositories. | Match installed OS and native libraries to advisories. |
| LW-07 Optional functionality | Inventory includes the CLI and user unit. The assessed system service had no DATA listener. | A qualified lab must agree the delivered boundary. |

Workstation evidence lives in `~/Virtual Machines/Linkwise Rocky9 Evidence`.
The `scans` subdirectory includes HTML, XCCDF and generated remediation
scripts. The [automation](../../tools/rocky9-vm/README.md) captures the creation,
build, hardening and assessment phases. The complete clean-room replay of the
consolidated scripts is pending. NIAP validation requires independent evaluation.

The [evaluator report](niap-evaluator-report.md) records independent-style
retests and verdicts for the draft security objectives.
