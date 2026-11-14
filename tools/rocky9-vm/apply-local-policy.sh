#!/bin/bash
set -euo pipefail
. /etc/os-release
[[ $ID == rocky && $VERSION_ID == 9.* ]]
# Keep the local PAM profile and remove permissive password options.
if [[ ! -d /etc/authselect/custom/linkwise ]]; then
    authselect create-profile linkwise -b minimal
fi
for template in /etc/authselect/custom/linkwise/{system-auth,password-auth}; do
    sed -i -E '/pam_unix\.so/s/[[:blank:]]+nullok//g; /password.*pam_unix\.so/{ /rounds=/! s/(sha512)([[:blank:]]|$)/\1 rounds=100000\2/; }' "$template"
done
authselect select custom/linkwise with-faillock
semanage fcontext -a -t faillog_t '/var/log/faillock(/.*)?' ||
    semanage fcontext -m -t faillog_t '/var/log/faillock(/.*)?'
restorecon -R /var/log/faillock
sed -i -E 's/^#([[:blank:]]*auth[[:blank:]]+required[[:blank:]]+pam_wheel\.so[[:blank:]]+use_uid)/\1/' /etc/pam.d/su
grep -q '^FAIL_DELAY[[:blank:]]' /etc/login.defs || printf 'FAIL_DELAY 4\n' >> /etc/login.defs
printf '* hard maxlogins 10\n' > /etc/security/limits.d/99-linkwise-stig.conf
install -d /etc/NetworkManager/conf.d
printf '[main]\ndns=default\n' > /etc/NetworkManager/conf.d/99-linkwise-stig.conf
if ! grep -q '^net.ipv4.conf.all.forwarding' /etc/sysctl.d/99-linkwise-stig.conf; then
    printf 'net.ipv4.conf.all.forwarding = 0\n' >> /etc/sysctl.d/99-linkwise-stig.conf
fi
printf 'kernel.kexec_load_disabled = 1\n' > /etc/sysctl.d/99-linkwise-kexec.conf
sysctl -p /etc/sysctl.d/99-linkwise-kexec.conf
authselect check
sshd -t
