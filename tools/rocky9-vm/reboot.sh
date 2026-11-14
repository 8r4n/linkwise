#!/bin/bash
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
work="$HOME/Virtual Machines/Linkwise Rocky9 Evidence"
utm="$HOME/Applications/UTM.app/Contents/MacOS/utmctl"
vm='Linkwise Rocky 9 ARM64'
# A cold start keeps the serial client in sync and tests the persisted boot entry.
"$here/ssh.sh" "sudo -S -p '' systemd-run --on-active=3s systemctl poweroff" < "$work/admin-password.txt"
for _ in {1..60}; do
    [[ $("$utm" status "$vm") == stopped ]] && break
    sleep 2
done
[[ $("$utm" status "$vm") == stopped ]]
python3 "$here/lw-rocky-first-boot.py" "$work" cold-boot.log "${1:-}" &
serial=$!
"$utm" start "$vm"
wait "$serial"
for _ in {1..20}; do
    if "$here/ssh.sh" true; then exit 0; fi
    sleep 2
done
exit 1
