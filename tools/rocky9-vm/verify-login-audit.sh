#!/bin/bash
set -euo pipefail
# Run over the administrator's SSH session so sudo inherits a regular login UID.
(( $(cat /proc/self/loginuid) >= 1000 && $(cat /proc/self/loginuid) != 4294967295 ))
auditctl -s
for path in /var/log/faillock /var/log/lastlog; do
    auditctl -l | grep -F -- "-F path=$path"
    before=$(date '+%m/%d/%Y %H:%M:%S')
    # Update timestamps without changing the login records.
    touch -m -- "$path"
    sleep 1
    read -r day time <<< "$before"
    records=$(ausearch --input-logs -ts "$day" "$time" -k logins -f "$path") || {
        # The installed auditd can reject EMAIL when sendmail is absent.
        # Match this test process in the raw audit events instead.
        python3 - "$path" "$$" <<'PY'
from pathlib import Path
import re
import sys

path, parent = sys.argv[1:]
entries = Path('/var/log/audit/audit.log').read_text().splitlines()
for entry in reversed(entries):
    if f' ppid={parent} ' in entry and ' comm="touch"' in entry and ' key="logins"' in entry:
        event = re.search(r'msg=audit\(([^)]+)\)', entry)[0]
        evidence = [line for line in entries if event in line]
        if any(f'name="{path}"' in line for line in evidence):
            print('\n'.join(evidence))
            break
else:
    raise SystemExit(f'No matching audit event for {path}')
PY
        continue
    }
    printf '%s\n' "$records"
done
