#!/bin/bash
set -euo pipefail
. /etc/os-release
[[ $ID == rocky && $VERSION_ID == 9.* ]]
printf '%s\n' 'deny perm=any all : all' > /etc/fapolicyd/rules.d/99-deny-everything.rules
chgrp fapolicyd /etc/fapolicyd/rules.d/99-deny-everything.rules
chmod 0644 /etc/fapolicyd/rules.d/99-deny-everything.rules
sed -i -E 's/^permissive[[:space:]]*=.*/permissive = 0/' /etc/fapolicyd/fapolicyd.conf
restorecon -R /etc/fapolicyd
fagenrules --load
systemctl enable --now fapolicyd
systemctl restart linkwise-modem
systemctl is-active fapolicyd linkwise-modem
