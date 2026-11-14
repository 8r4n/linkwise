#!/bin/bash
# Accept a GRUB PBKDF2 hash on stdin. BLS normal boot remains unrestricted.
set -euo pipefail
read -r hash
[[ $hash =~ ^grub\.pbkdf2\.sha512\.[0-9]+\.[[:xdigit:]]+\.[[:xdigit:]]+$ ]]
config=/boot/grub2/grub.cfg
backup=/boot/grub2/grub.cfg.pre-linkwise-auth
if [[ ! -e $backup ]]; then cp -p "$config" "$backup"; fi
install -m 0600 /dev/null /boot/grub2/linkwise-auth.cfg
printf 'set superusers="lwboot"\nexport superusers\npassword_pbkdf2 lwboot %s\n' "$hash" > /boot/grub2/linkwise-auth.cfg
cat > /etc/grub.d/02_linkwise_auth <<'GRUB'
#!/bin/sh
cat <<'CONFIG'
if [ -f ${prefix}/linkwise-auth.cfg ]; then
  source ${prefix}/linkwise-auth.cfg
fi
CONFIG
GRUB
chmod 0755 /etc/grub.d/02_linkwise_auth
restorecon /boot/grub2/linkwise-auth.cfg /etc/grub.d/02_linkwise_auth
grub2-mkconfig -o "$config"
grub2-script-check "$config"
grub2-script-check /boot/grub2/linkwise-auth.cfg
grep -q 'source ${prefix}/linkwise-auth.cfg' "$config"
grub2-editenv - unset menu_auto_hide
