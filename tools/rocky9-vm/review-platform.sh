#!/bin/bash
# Capture platform evidence without printing secret material.
set -euo pipefail
date -u
uname -rm
rpm -q rocky-release kernel-core openssl-fips-provider cryptsetup
sha256sum /usr/lib64/ossl-modules/fips.so "/boot/vmlinuz-$(uname -r)"
fips-mode-setup --check
update-crypto-policies --show
openssl list -providers
mokutil --sb-state || true
printf '\nLocal firewall zones and default target\n'
firewall-cmd --get-active-zones
zone=$(firewall-cmd --get-default-zone)
firewall-cmd --zone="$zone" --list-all
printf '\nAudit storage\n'
findmnt /var/log/audit
df -h /var/log/audit
grep -E '^(max_log_file|num_logs|space_left|admin_space_left|disk_full_action)' /etc/audit/auditd.conf
printf '\nLocal account inventory\n'
getent passwd | awk -F: '$3==0 || ($3>=1000 && $3<60000) {print $1,$3,$6,$7}'
printf '\nAvailable security updates\n'
dnf -q --refresh updateinfo list --security --updates
