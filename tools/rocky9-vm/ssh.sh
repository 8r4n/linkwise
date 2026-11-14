#!/bin/bash
set -e
d="$HOME/Virtual Machines/Linkwise Rocky9 Evidence"
exec ssh -i "$d/admin-rsa" -o IdentitiesOnly=yes -o BatchMode=yes -o ConnectTimeout=5 -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile="$d/known_hosts" -p 2224 lwadmin@127.0.0.1 "$@"
