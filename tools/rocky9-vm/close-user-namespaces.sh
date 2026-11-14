#!/bin/bash
set -euo pipefail
# This dedicated bench runs no rootless containers or user-namespace workloads.
printf 'user.max_user_namespaces = 0\n' > /etc/sysctl.d/99-linkwise-userns.conf
restorecon /etc/sysctl.d/99-linkwise-userns.conf
sysctl -p /etc/sysctl.d/99-linkwise-userns.conf
if runuser -u lwadmin -- unshare -U true; then
    echo 'Unprivileged user namespace creation unexpectedly succeeded' >&2
    exit 1
fi
systemctl is-active linkwise-modem
