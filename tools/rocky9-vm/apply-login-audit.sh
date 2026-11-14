#!/bin/bash
# Match the full Rocky profile's syscall audit checks.
set -euo pipefail
rules=/etc/audit/rules.d/55-linkwise-login.rules
install -m 0600 /dev/null "$rules"
for arch in b32 b64; do
    for path in /var/log/faillock /var/log/lastlog; do
        printf -- '-a always,exit -F arch=%s -F path=%s -F perm=wa -F auid>=1000 -F auid!=unset -k logins\n' "$arch" "$path"
    done
done > "$rules"
restorecon "$rules"
# Immutable rules take effect at the next boot.
augenrules
