#!/bin/bash
set -euo pipefail
unit=linkwise-modem.service
pid=$(systemctl show "$unit" -p MainPID --value)
test "$pid" -gt 0
ps -o user=,label= -p "$pid"
[[ $(cat /proc/sys/crypto/fips_enabled) == 1 ]]
[[ $(getenforce) == Enforcing ]]
[[ $(systemctl is-active auditd fapolicyd usbguard | sort -u) == active ]]
ip=$(ip -4 -o route get 192.0.2.1 | awk '{for (i=1;i<=NF;i++) if ($i=="src") print $(i+1)}')
python3 - "$ip" <<'PY'
import errno
import socket
import sys
ip = sys.argv[1]
assert ip and ip != "127.0.0.1"
for port in (8300, 8301):
    try:
        with socket.create_connection((ip, port), timeout=3):
            raise SystemExit(f"Unexpected listener at {ip}:{port}")
    except OSError as error:
        if error.errno != errno.ECONNREFUSED:
            raise
    print(f"Connection refused at {ip}:{port}")
PY
for mount in / /home /tmp /var /var/tmp /var/log /var/log/audit; do
    source=$(findmnt -n -o SOURCE "$mount")
    lsblk -snr -o TYPE "$source" | grep -x crypt >/dev/null
    printf 'LUKS-backed mount %s\n' "$mount"
done
swap=$(swapon --show=NAME --noheadings)
test -n "$swap"
lsblk -snr -o TYPE "$swap" | grep -x crypt >/dev/null
printf 'LUKS-backed swap %s\n' "$swap"
