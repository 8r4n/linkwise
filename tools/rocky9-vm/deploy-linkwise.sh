#!/bin/bash
set -euo pipefail
stage=${1:?stage directory}
package=$(find "$stage" -maxdepth 1 -name '*.rpm' -print -quit)
test -n "$package"
test "$(getenforce)" = Enforcing
test "$(cat /proc/sys/crypto/fips_enabled)" = 1
rpmkeys --import "$stage/bench-public.asc"
rpmkeys --checksig "$package"
dnf -y --setopt=localpkg_gpgcheck=1 install "$package"
install -d -m 0755 /etc/linkwise
install -m 0644 /dev/null /etc/linkwise/station.json
if ! modprobe snd-aloop; then
    dnf -y install kernel-modules-extra
    modprobe snd-aloop
fi
printf 'snd-aloop\n' > /etc/modules-load.d/linkwise-loopback.conf
cat >/etc/linkwise/station.json <<'JSON'
{"version":1,"callsign":"W1AW","input":{"name":"sysdefault:CARD=Loopback","channel":0},"output":{"name":"hw:CARD=Loopback,DEV=1","channel":0}}
JSON
restorecon -Rv /etc/linkwise
/usr/bin/linkwise-modemd --check-config /etc/linkwise/station.json
systemctl enable --now linkwise-modem.service
