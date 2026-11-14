#!/bin/bash
set -euo pipefail
d="$HOME/Virtual Machines/Linkwise Rocky9 Evidence"
u="$HOME/Applications/UTM.app/Contents/MacOS/utmctl"
vm='Linkwise Rocky 9 ARM64'
for i in {1..110}; do
 [[ $("$u" status "$vm") == stopped ]] && break
 sleep 10
done
[[ $("$u" status "$vm") == stopped ]]
grep -aiE 'Installation complete|Powering [Oo]ff|Power down' "$d/install-live.log" >/dev/null
python3 - <<'PY'
import plistlib
from pathlib import Path
b=Path.home()/'Virtual Machines'/'Linkwise Rocky 9 ARM64.utm'
p=b/'config.plist'; c=plistlib.loads(p.read_bytes())
c['Drive']=c['Drive'][:1]; c['QEMU']['AdditionalArguments']=[]
p.write_bytes(plistlib.dumps(c))
PY
osascript -e 'tell application "UTM" to reload configuration of virtual machine "Linkwise Rocky 9 ARM64"'
python3 "$(dirname "$0")/lw-rocky-first-boot.py" "$d" &
serial=$!
"$u" start "$vm"
wait "$serial"
