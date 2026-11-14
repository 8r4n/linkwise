# Rocky 9 STIG disposition register

The selected baseline is the full Rocky Security Guide STIG profile. The
November 13 Pacific scan used package `scap-security-guide-0.1.82-2.el9_8.rocky.1.1`
and recorded 397 passes, 31 failures, 10 unchecked and 38 inapplicable.
This register preserves scan verdicts and records the evidence needed for
closure. The native DISA RHEL scan is reference evidence. Retain its outputs
and record formal approval for each Rocky equivalency or inapplicability.

## Project disposition for HF-only production

The project owner designated the 31 failed and 10 unchecked findings from
the latest Rocky scan as not applicable to the HF-only production profile.
This is the project disposition for all 41 open scan findings. The original
scanner results and per-rule evidence below are retained for reviewer
traceability. Independent acceptance and rule-specific rationale remain
unverified. The separate Common Criteria, Secure Boot, CMVP, and release
verification gates retain their recorded status.

Rule IDs below omit the common `xccdf_org.ssgproject.content_rule_` prefix.

| Rule | Scan | Closure evidence or action |
| --- | --- | --- |
| `aide_scan_notification` | fail | Verify site policy and integration. |
| `fips_custom_stig_sub_policy` | fail | Validate Rocky coverage. |
| `configure_crypto_policy` | fail | Validate Rocky coverage. |
| `crypto_policy_not_overridden` | notchecked | Validate Rocky coverage. |
| `installed_OS_is_vendor_supported` | fail | Validate Rocky coverage. |
| `encrypt_partitions` | notchecked | LUKS ancestry verified for seven data mounts and swap. Review EFI and boot boundary. |
| `package_subscription-manager_installed` | fail | Validate Rocky coverage. |
| `security_patches_up_to_date` | notchecked | Refreshed DNF update and advisory checks returned no available updates. Confirm approved repository coverage. |
| `banner_etc_issue` | fail | Verify site policy and integration. |
| `package_opensc_installed` | fail | Verify site policy and integration. |
| `package_pcsc-lite_installed` | fail | Verify site policy and integration. |
| `install_smartcard_packages` | fail | Verify site policy and integration. |
| `service_pcscd_enabled` | fail | Verify site policy and integration. |
| `configure_opensc_card_drivers` | fail | Verify site policy and integration. |
| `accounts_authorized_local_users` | fail | Verify site policy and integration. |
| `account_temp_expire_date` | notchecked | Review provisioned account inventory against the approved roster and account expiration policy. |
| `accounts_user_home_paths_only` | notchecked | Compare regular accounts and home directories against the approved roster. |
| `grub2_pti_argument` | fail | Validate Rocky coverage. |
| `grub2_admin_username` | fail | GRUB prompted for credentials and refused unauthenticated edit access. Normal boot succeeded. Rocky check still fails for the deployed configuration. Verify authorized recovery and obtain a rule-level disposition. |
| `grub2_password` | fail | GRUB prompted for credentials and refused unauthenticated edit access. Normal boot succeeded. Rocky check still fails for the deployed configuration. Verify authorized recovery and obtain a rule-level disposition. |
| `rsyslog_encrypt_offload_actionsendstreamdriverauthmode` | fail | Verify site policy and integration. |
| `rsyslog_encrypt_offload_actionsendstreamdrivermode` | fail | Verify site policy and integration. |
| `rsyslog_encrypt_offload_defaultnetstreamdriver` | fail | Verify site policy and integration. |
| `rsyslog_remote_access_monitoring` | fail | Verify site policy and integration. |
| `rsyslog_remote_loghost` | fail | Verify site policy and integration. |
| `network_configure_name_resolution` | fail | Verify site policy and integration. |
| `configured_firewalld_default_deny` | notchecked | Active zone uses DROP, SSH only, and forwarding disabled. Verify approved management exception. |
| `package_libreswan_installed` | fail | Verify site policy and integration. |
| `libreswan_approved_tunnels` | notchecked | Verify site policy and integration. |
| `sysctl_user_max_user_namespaces_no_remediation` | pass after remediation | Set the limit to zero, verified that a regular user could not create a user namespace, and confirmed the setting persisted after cold boot. |
| `selinux_context_elevation_for_sudo` | fail | Verify local control after remediation. |
| `package_postfix_installed` | fail | Verify site policy and integration. |
| `package_s-nail_installed` | fail | Verify site policy and integration. |
| `postfix_client_configure_mail_alias` | fail | Verify site policy and integration. |
| `chronyd_or_ntpd_set_maxpoll` | fail | Verify site policy and integration. |
| `chronyd_server_directive` | fail | Verify site policy and integration. |
| `ssh_keys_passphrase_protected` | notchecked | Review administrator key protection. The bench automation uses an unencrypted dedicated private key. |
| `sssd_certificate_verification` | fail | Verify site policy and integration. |
| `sssd_enable_certmap` | fail | Verify site policy and integration. |
| `sssd_enable_smartcards` | fail | Verify site policy and integration. |
| `sssd_has_trust_anchor` | notchecked | Verify site policy and integration. |
| `audit_rules_login_events_faillock` | pass after remediation | Rules loaded in immutable mode after cold boot. Audit event 1838 recorded the directed access under administrator login UID 1000. |
| `audit_rules_login_events_lastlog` | pass after remediation | Rules loaded in immutable mode after cold boot. Audit event 1839 recorded the directed access under administrator login UID 1000. |
| `auditd_audispd_configure_sufficiently_large_partition` | notchecked | Dedicated 960 MiB audit filesystem observed. Verify capacity against retention and forwarding requirements. |

For Rocky coverage decisions, identify the RHEL control intent and record
the Rocky mechanism, operational evidence and reviewer decision. For site
controls, test against the actual approved service or policy. For local
changes, rerun the full profile and verify boot, SSH, SELinux and Linkwise
operation. For external assessment, retain the original scan evidence until the reviewer records a disposition.

## Production HF-only applicability review

Production traffic is HF-only. Administrative and maintenance connectivity
remain to be defined. The current VM uses SSH, IP networking and online
repositories. Its scan counts remain the observed baseline. Any exception
requires the exact rule, deployment evidence, and an approved disposition.

| Controls | Proposed disposition and verification |
| --- | --- |
| Libreswan and approved tunnels | Review for inapplicability if production has no IPsec role or maintenance tunnel. Verify topology, installed services, routes, and IPsec policy. |
| DNS and network time | Review the network-specific checks if deployment has no DNS or NTP path, including maintenance. Document how trusted time, drift and audit timestamps are verified. |
| Postfix, mail client and AIDE notification | Review the SMTP-specific mechanism if there is no SMTP path. Demonstrate the approved local alert or offline delivery process, including AIDE alerts and audit low-space alerts. Linkwise's HF payload transport is a separate capability. |
| Remote audit forwarding and remote-access monitoring | Review forwarding and remote-access rules separately. If no collector is reachable, propose protected local retention and authenticated offline export. Trigger an event and verify its retention and recovery. SSH or other maintenance access still needs monitoring if enabled. |
| Smart-card packages and SSSD | Check the actual privileged access policy. Local console administration can still require MFA. Verify enrollment and login on the chosen mechanism before adjudicating implementation-specific package checks. |
| Boot, encryption, local privileges and account policy | Keep these in assessment. Verify boot integrity, storage and cryptographic coverage, local authentication, SELinux enforcement, and patch delivery. Review Rocky and ARM-specific substitutions on their own evidence. |

If production transmits or forwards mail over HF, scope those paths in the
Security Target and add evaluator tests for their interfaces and privileges.
Current modem evidence covers the receive-only candidate.
See [NIST SP 800-53B](https://csrc.nist.gov/pubs/sp/800/53/b/upd1/final)
for baseline tailoring and compensating controls.

### HF isolation bench run

A temporary systemd drop-in isolated the installed modem in a network
namespace containing only loopback. The namespace had no IPv4 or IPv6
default route. TCP 8300 listened locally, TCP 8301 refused a connection, and
TCP probes for DNS, SMTP and TLS log forwarding returned network unreachable.
The installed service decoded three BW500 level-1 frames. The script then
restored the original bench networking. These results cover the service
namespace. The Rocky host still has an IP maintenance path, so retain the
host-level findings pending deployment-mode evidence.
