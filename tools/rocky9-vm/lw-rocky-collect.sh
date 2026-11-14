#!/bin/bash
set -euo pipefail
dir=${1:?output-directory}
mkdir -p "$dir"
exec > >(tee "$dir/host.log") 2>&1
cat /etc/rocky-release
uname -a
rpm -q kernel-core openssl-libs openssl-fips-provider cryptsetup selinux-policy-targeted || :
printf 'Linux FIPS flag: '; cat /proc/sys/crypto/fips_enabled
update-crypto-policies --show
fips-mode-setup --check || :
openssl list -providers
getenforce
lsblk -o NAME,FSTYPE,TYPE,MOUNTPOINTS
findmnt --real
swapon --show
while read -r device; do
    cryptsetup luksDump "$device" | sed -n '/^Version:/p;/^[[:space:]]*PBKDF:/p;/^[[:space:]]*cipher:/p'
done < <(lsblk -rpn -o NAME,FSTYPE | awk '$2=="crypto_LUKS"{print $1}')
systemctl is-active sshd firewalld auditd
sshd -T | grep -E '^(permitrootlogin|passwordauthentication|kbdinteractiveauthentication) '
auditctl -s
set +e
dnf -q check-update --refresh
result=$?
set -e
printf 'dnf check-update exit code: %s\n' "$result"
case $result in 0|100) ;; *) exit "$result";; esac
