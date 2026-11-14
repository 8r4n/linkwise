#!/bin/bash
# Selected RHEL 9 STIG controls adapted to the isolated Rocky guest.
set -euo pipefail
. /etc/os-release
[[ $ID == rocky && $VERSION_ID == 9.* ]]
[[ $(getenforce) == Enforcing && $(cat /proc/sys/crypto/fips_enabled) == 1 ]]
dnf -y install gnutls-utils nss-tools rsyslog-gnutls
grep -q '^localpkg_gpgcheck=1$' /etc/dnf/dnf.conf ||
    sed -i '/^\[main\]$/a localpkg_gpgcheck=1' /etc/dnf/dnf.conf
printf '%s\n' 'Defaults timestamp_timeout=0' 'Defaults !rootpw, !targetpw, !runaspw' > /etc/sudoers.d/90-linkwise-stig
chmod 0440 /etc/sudoers.d/90-linkwise-stig
visudo -cf /etc/sudoers
systemctl mask --force debug-shell.service ctrl-alt-del.target
install -d /etc/systemd/system.conf.d
printf '[Manager]\nCtrlAltDelBurstAction=none\n' > /etc/systemd/system.conf.d/90-linkwise-stig.conf
grubby --update-kernel=ALL --args="boot=UUID=$(findmnt -no UUID /boot) audit_backlog_limit=8192 init_on_free=1"
cat >/etc/sysctl.d/99-linkwise-stig.conf <<'SYSCTL'
net.ipv4.ip_forward = 0
net.ipv4.conf.all.accept_redirects = 0
net.ipv4.conf.default.accept_redirects = 0
net.ipv4.conf.all.accept_source_route = 0
net.ipv4.conf.default.accept_source_route = 0
net.ipv4.conf.all.log_martians = 1
net.ipv4.conf.default.log_martians = 1
net.ipv4.conf.all.rp_filter = 1
net.ipv4.conf.default.rp_filter = 1
net.ipv4.conf.all.send_redirects = 0
net.ipv4.conf.default.send_redirects = 0
net.ipv4.icmp_echo_ignore_broadcasts = 1
net.ipv4.icmp_ignore_bogus_error_responses = 1
net.ipv4.tcp_syncookies = 1
net.ipv6.conf.all.forwarding = 0
net.ipv6.conf.all.accept_ra = 0
net.ipv6.conf.default.accept_ra = 0
net.ipv6.conf.all.accept_redirects = 0
net.ipv6.conf.default.accept_redirects = 0
net.ipv6.conf.all.accept_source_route = 0
net.ipv6.conf.default.accept_source_route = 0
SYSCTL
sysctl --system
for module in atm can firewire-core sctp tipc bluetooth cramfs usb-storage; do
    printf 'install %s /bin/false\nblacklist %s\n' "$module" "$module"
done > /etc/modprobe.d/99-linkwise-stig.conf
python3 - <<'PY'
from pathlib import Path
fstab = Path("/etc/fstab")
volumes = {"/boot", "/boot/efi", "/home", "/tmp", "/var", "/var/tmp", "/var/log", "/var/log/audit"}
noexec = volumes - {"/boot", "/var"}
lines = []
for line in fstab.read_text().splitlines():
    fields = line.split()
    if len(fields) >= 6 and not line.lstrip().startswith("#") and fields[1] in volumes:
        options = fields[3].split(",") + ["nodev", "nosuid"]
        if fields[1] in noexec:
            options.append("noexec")
        fields[3] = ",".join(dict.fromkeys(options))
        line = "\t".join(fields)
    if len(fields) >= 6 and fields[1] == "/dev/shm":
        continue
    lines.append(line)
lines.append("tmpfs\t/dev/shm\ttmpfs\tdefaults,nodev,nosuid,noexec\t0\t0")
fstab.write_text("\n".join(lines) + "\n")
PY
findmnt --verify
restorecon /etc/fstab /etc/sudoers.d/90-linkwise-stig
