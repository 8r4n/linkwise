#!/bin/bash
# Configure loopback-only mobile fixtures on a dedicated Rocky test guest.
set -euo pipefail
stage=${1:?bench directory}
test ! -e /etc/linkwise-mail
install -d -m 0750 -o root -g linkwise-mail /etc/linkwise-mail
install -m 0640 -o root -g linkwise-mail "$stage/tls/server.key" /etc/linkwise-mail/server.key
install -m 0644 "$stage/tls/server.pem" /etc/linkwise-mail/server.pem
umask 077
printf '%s\n' LINKWISE_CALLSIGN=KN6MQS LINKWISE_SMTP_USER=bench LINKWISE_SMTP_PASSWORD=fixture >/etc/linkwise-mail/smtp.env
printf '%s\n' LINKWISE_IMAP_USER=bench LINKWISE_IMAP_PASSWORD=fixture >/etc/linkwise-mail/imap.env
install -m 0644 "$stage/reference/vara/fixtures/level4/bw500-level4.json" /etc/linkwise/bw500-level4.json
cp -pn /etc/linkwise/station.json /etc/linkwise/station.json.pre-mobile
python3 - <<'PY'
from pathlib import Path
import json
p = Path('/etc/linkwise/station.json')
config = json.loads(p.read_text())
config.update(bandwidth=500, callsign="KN6MQS", bw500_profile="bw500-level4.json")
config["output"]["name"] = "sysdefault:CARD=Loopback"
p.write_text(json.dumps(config) + '\n')
PY
restorecon -R /etc/linkwise /etc/linkwise-mail
modprobe snd-aloop
/usr/bin/linkwise-modemd --check-config /etc/linkwise/station.json
systemctl enable --now linkwise-modem linkwise-mail-smtp linkwise-mail-imap
systemctl is-active linkwise-modem linkwise-mail-smtp linkwise-mail-imap
ss -ltnp
