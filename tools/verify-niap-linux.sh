#!/usr/bin/env bash
# Read-only checks of the installed receive-only candidate on Fedora 45 or Rocky Linux 9 ARM64.
set -euo pipefail

rpm_path=${1:?usage: sudo bash tools/verify-niap-linux.sh PATH_TO_RPM}
unit=linkwise-modem.service
binary=/usr/bin/linkwise-modemd
config=/etc/linkwise/station.json

. /etc/os-release
[[ $(uname -m) == aarch64 ]]
case "$ID:$VERSION_ID" in
    fedora:45) ;;
    rocky:9|rocky:9.*)
        [[ $(cat /proc/sys/crypto/fips_enabled) == 1 ]]
        [[ $(update-crypto-policies --show) == FIPS ]]
        ;;
    *) printf 'Unsupported assessment host: %s %s\n' "$ID" "$VERSION_ID" >&2; exit 1 ;;
esac
[[ $(getenforce) == Enforcing ]]
printf 'Host %s, kernel %s, SELinux Enforcing\n' "$PRETTY_NAME" "$(uname -r)"
rpm -q selinux-policy-targeted
sha256sum "$rpm_path"
rpm -K --nosignature "$rpm_path"
[[ $(rpm -q --queryformat '%{NEVRA}' linkwise-modem) == $(rpm -qp --queryformat '%{NEVRA}' "$rpm_path") ]]
# Match the running binary to the candidate package, including for same-version rebuilds.
expected=$(rpm -qp --dump "$rpm_path" | awk '$1=="/usr/bin/linkwise-modemd"{print $4}')
[[ -n $expected && $expected == "$(sha256sum "$binary" | cut -d' ' -f1)" ]]
rpm -V linkwise-modem

pid=$(systemctl show "$unit" -p MainPID --value)
[[ $pid =~ ^[1-9][0-9]*$ ]]
[[ $(readlink "/proc/$pid/exe") == "$binary" ]]
[[ $expected == "$(sha256sum "/proc/$pid/exe" | cut -d' ' -f1)" ]]
[[ $(ps -o label= -p "$pid") == *:linkwise_modem_t:* ]]
stat -c '%C %n' "$binary" "$config"
[[ $(stat -c %C "$binary") == *:linkwise_modem_exec_t:* ]]
[[ $(stat -c %C "$config") == *:linkwise_modem_etc_t:* ]]
[[ $(stat -c %u "$config") == 0 ]]
[[ $(stat -c %u /etc/linkwise) == 0 ]]
for file in /etc/linkwise "$config"; do
    mode=$(stat -c %a "$file")
    (( (8#$mode & 022) == 0 ))
done
awk '/^Uid:/ {if ($2 > 0 && $3 > 0) uid=1} /^NoNewPrivs:/ {if ($2==1) nnp=1}
     END {exit !(uid && nnp)}' "/proc/$pid/status"
[[ $(systemctl show "$unit" -p DevicePolicy --value) == closed ]]
[[ $(systemctl show "$unit" -p ProtectSystem --value) == strict ]]
semanage port -l | awk '
    $1 == "linkwise_modem_port_t" && $2 == "tcp" {
        line=$0
        gsub(/[,[:space:]]+/, " ", line)
        if (line ~ /(^| )8300( |$)/ && line ~ /(^| )8301( |$)/) found=1
    }
    END {exit !found}'
python3 - "$config" <<'PY'
import json
import socket
import sys

with open(sys.argv[1], encoding="utf-8") as source:
    config = json.load(source)
assert config.get("bandwidth") is None
assert config.get("bw500_profile") is None
assert config.get("command_port", 8300) == 8300
assert config.get("data_port", 8301) == 8301
with socket.create_connection(("127.0.0.1", 8300), timeout=3) as stream:
    with stream.makefile("rb") as reply:
        status = json.loads(reply.readline(4096))
assert status["ready"] is True
assert isinstance(status["received_frames"], int)
with socket.socket() as stream:
    stream.settimeout(3)
    try:
        stream.connect(("127.0.0.1", 8301))
    except ConnectionRefusedError:
        pass
    else:
        raise SystemExit("Unexpected service on TCP 8301")
print("Receive status available on loopback; DATA port refused")
PY
ss -H -ltnp | awk -v owner="pid=$pid," '
    index($0, owner) { if ($4 != "127.0.0.1:8300") exit 1; ++count }
    END {if (count != 1) exit 1}'
printf 'Installed receive-only configuration checks passed.\n'
