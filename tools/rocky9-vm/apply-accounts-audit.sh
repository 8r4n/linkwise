#!/bin/bash
set -euo pipefail
. /etc/os-release
[[ $ID == rocky && $VERSION_ID == 9.* ]]
# This guest uses local accounts and SSH public-key access, so select local PAM.
authselect select minimal with-faillock --force
install -d -m 0755 /var/log/faillock
restorecon -R /var/log/faillock
cat >/etc/security/faillock.conf <<'CONF'
audit
deny = 3
fail_interval = 900
unlock_time = 0
even_deny_root
dir = /var/log/faillock
CONF
install -d /etc/security/pwquality.conf.d
cat >/etc/security/pwquality.conf.d/99-linkwise-stig.conf <<'CONF'
minlen = 15
difok = 8
dcredit = -1
ucredit = -1
lcredit = -1
ocredit = -1
minclass = 4
maxrepeat = 3
maxclassrepeat = 4
dictcheck = 1
enforce_for_root
retry = 3
CONF
chage -m 1 -M 60 -I 35 lwadmin
useradd -D -f 35
sed -i -E 's/^PASS_MAX_DAYS[[:space:]].*/PASS_MAX_DAYS 60/;s/^PASS_MIN_DAYS[[:space:]].*/PASS_MIN_DAYS 1/' /etc/login.defs
printf '%s\n' 'readonly TMOUT=600' 'export TMOUT' >/etc/profile.d/99-linkwise-timeout.sh
chmod 0600 /etc/ssh/sshd_config.d/01-linkwise.conf /etc/ssh/ssh_host_*_key
sshd -t
find /root -maxdepth 1 -type f -name '.*' -exec chmod go-wx,o-r {} +
dnf -y install audispd-plugins
bash "$(dirname "$0")/apply-login-audit.sh"
printf '%s\n' '-e 2' >/etc/audit/rules.d/99-finalize.rules
chmod 0600 /etc/audit/rules.d/55-linkwise-login.rules /etc/audit/rules.d/99-finalize.rules
sed -i -E '/^freq[[:space:]]*=/s/=.*/= 100/; /^name_format[[:space:]]*=/s/=.*/= HOSTNAME/; /^space_left[[:space:]]*=/s/=.*/= 25%/; /^space_left_action[[:space:]]*=/s/=.*/= EMAIL/; /^admin_space_left[[:space:]]*=/s/=.*/= 10%/; /^admin_space_left_action[[:space:]]*=/s/=.*/= SINGLE/; /^disk_full_action[[:space:]]*=/s/=.*/= HALT/; /^disk_error_action[[:space:]]*=/s/=.*/= HALT/' /etc/audit/auditd.conf
restorecon -R /etc/security /etc/audit /etc/profile.d
authselect check
usbguard generate-policy > /etc/usbguard/rules.conf
chmod 0600 /etc/usbguard/rules.conf
sed -i -E 's/^AuditBackend=.*/AuditBackend=LinuxAudit/' /etc/usbguard/usbguard-daemon.conf
restorecon -R /etc/usbguard
systemctl enable --now usbguard
# Initialize after applying filesystem and package controls.
aide --init
install -m 0600 /var/lib/aide/aide.db.new.gz /var/lib/aide/aide.db.gz
restorecon -R /var/lib/aide
