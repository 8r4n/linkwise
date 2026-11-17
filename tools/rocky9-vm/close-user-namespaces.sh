#!/bin/bash
set -euo pipefail
# This dedicated bench runs no rootless containers or user-namespace workloads.
case "${1:-}" in
    ''|--live-image) ;;
    *) exit 2 ;;
esac
printf 'user.max_user_namespaces = 0\n' > /etc/sysctl.d/99-linkwise-userns.conf
if [[ ${1:-} == --live-image ]]; then exit 0; fi
restorecon /etc/sysctl.d/99-linkwise-userns.conf
sysctl -p /etc/sysctl.d/99-linkwise-userns.conf
if runuser -u lwadmin -- unshare -U true; then
    echo 'Unprivileged user namespace creation unexpectedly succeeded' >&2
    exit 1
fi
systemctl is-active linkwise-modem
