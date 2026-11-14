#!/bin/bash
set -euo pipefail
unit=linkwise-modem.service
install -d -m 0755 "/etc/systemd/system/$unit.d"
printf '[Service]\nMemoryMax=128M\nCPUQuota=100%%\nTasksMax=64\n' > "/etc/systemd/system/$unit.d/90-evaluation-limits.conf"
restorecon -R "/etc/systemd/system/$unit.d"
systemctl daemon-reload
systemctl restart "$unit"
systemctl show "$unit" -p CPUQuotaPerSecUSec -p MemoryMax -p TasksMax
