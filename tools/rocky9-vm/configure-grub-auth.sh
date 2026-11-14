#!/bin/bash
# Host-side credential generation and installation.
set -euo pipefail
umask 077
here=$(cd "$(dirname "$0")" && pwd)
work="$HOME/Virtual Machines/Linkwise Rocky9 Evidence"
secret="$work/grub-password.txt"
if [[ ! -s "$secret" ]]; then openssl rand -hex 24 > "$secret"; fi
hash=$({ cat "$secret"; cat "$secret"; } | "$here/ssh.sh" grub2-mkpasswd-pbkdf2 |
    sed -nE 's/.*(grub\.pbkdf2\.sha512\.[[:alnum:].]+).*/\1/p')
test -n "$hash"
{ cat "$work/admin-password.txt"; printf '%s\n' "$hash"; } |
    "$here/ssh.sh" "sudo -S -p '' bash /home/lwadmin/rocky-assessment/apply-grub-auth.sh"
