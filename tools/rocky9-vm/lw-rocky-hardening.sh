#!/bin/bash
set -euo pipefail
dnf -y --refresh upgrade
dnf -y install openscap-scanner scap-security-guide policycoreutils-python-utils audit firewalld chrony aide rsyslog usbguard fapolicyd
systemctl enable --now firewalld chronyd rsyslog
systemctl enable auditd
test "$(getenforce)" = Enforcing
test "$(cat /proc/sys/crypto/fips_enabled)" = 1
test "$(update-crypto-policies --show)" = FIPS
