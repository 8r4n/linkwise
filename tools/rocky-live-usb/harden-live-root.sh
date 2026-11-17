#!/usr/bin/env bash
# Prepare a mounted Rocky live filesystem for the encrypted USB build.
set -euo pipefail
umask 077
here=$(cd "$(dirname "$0")" && pwd)
root=$(realpath "${1:?mounted live filesystem}")
login=$(realpath "${2:?operator password file}")
[[ $root != / && -f $root/etc/rocky-release ]]
mountpoint -q "$root"
[[ $(stat -c '%a' "$login") == 600 ]]
if ! rpm -q usbguard fapolicyd >/dev/null; then
    echo "Install USBGuard and fapolicyd on the builder so its SELinux policy accepts the target labels." >&2
    exit 1
fi
case "$(uname -m)" in
    aarch64) grub_cdboot=grub2-efi-aa64-cdboot ;;
    x86_64) grub_cdboot=grub2-efi-x64-cdboot ;;
    *) echo "Unsupported Rocky live architecture." >&2; exit 2 ;;
esac
# The offline target's full policy is loaded only after this transaction.
# Apply its file contexts below, once the target policy store is complete.
dnf -y --installroot="$root" --releasever=9 \
    --setopt=install_weak_deps=False --setopt=tsflags=nodocs,nocontexts install \
    "$grub_cdboot" \
    aide audit audispd-plugins authselect chrony cryptsetup crypto-policies-scripts dracut-fips \
    fapolicyd fapolicyd-selinux firewalld gnutls-utils mutt nss-tools openscap-scanner openssl openssl-fips-provider \
    policycoreutils-python-utils rsyslog rsyslog-gnutls scap-security-guide sudo usbguard usbguard-selinux zstd

inside() { chroot "$root" "$@"; }
contexts="$root/etc/selinux/targeted/contexts/files/file_contexts"
policies=("$root"/etc/selinux/targeted/policy/policy.*)
test "${#policies[@]}" = 1
test -f "${policies[0]}"
if ! inside id lwadmin >/dev/null 2>&1; then
    inside useradd -m -G wheel -s /bin/bash lwadmin
fi
setfiles -F -c "${policies[0]}" -r "$root" "$contexts" \
    "$root/etc/passwd" "$root/etc/shadow" "$root/etc/group" "$root/etc/gshadow"
hash=$(inside openssl passwd -6 -stdin < "$login")
printf 'lwadmin:%s\n' "$hash" | inside chpasswd --encrypted
unset hash
inside passwd -l root
inside update-crypto-policies --no-reload --set FIPS
touch "$root/etc/system-fips"
sed -i -E 's/^SELINUX=.*/SELINUX=enforcing/' "$root/etc/selinux/config"

install -d "$root/run/linkwise-build"
# The partitionless console root may have no fstab before hardening.
if [[ ! -e $root/etc/fstab ]]; then
    install -m 0644 /dev/null "$root/etc/fstab"
fi
for name in apply-stig.sh apply-accounts-audit.sh apply-login-audit.sh apply-local-policy.sh; do
    install -m 0700 "$here/../rocky9-vm/$name" "$root/run/linkwise-build/"
done
for name in apply-stig.sh apply-accounts-audit.sh apply-local-policy.sh; do
    inside bash "/run/linkwise-build/$name" --live-image
done
if [[ ! -e $root/usr/lib/systemd/system/lightdm.service ]]; then
    install -m 0700 "$here/../rocky9-vm/close-user-namespaces.sh" "$root/run/linkwise-build/"
    inside bash /run/linkwise-build/close-user-namespaces.sh --live-image
    unlink "$root/run/linkwise-build/close-user-namespaces.sh"
fi
for name in apply-stig.sh apply-accounts-audit.sh apply-login-audit.sh apply-local-policy.sh; do
    unlink "$root/run/linkwise-build/$name"
done
rmdir "$root/run/linkwise-build"
install -m 0644 "$here/../test-varac-chat.py" "$root/usr/share/linkwise-live/"
install -m 0644 "$here/live-controls.sh" "$root/usr/share/linkwise-live/"
install -Dm0755 "$here/shutdown-memory/linkwise-shred" "$root/usr/sbin/linkwise-shred"
install -Dm0644 "$here/shutdown-memory/usb-identify.sh" "$root/usr/libexec/linkwise-usb-identify"
install -Dm0755 "$here/linkwise-mutt" "$root/usr/bin/linkwise-mutt"
install -Dm0644 "$here/muttrc" "$root/etc/linkwise-live/muttrc"

if [[ -e $root/usr/lib/systemd/system/livesys.service ]]; then
    systemctl --root="$root" disable livesys.service livesys-late.service
fi
systemctl --root="$root" mask sshd.service udisks2.service ModemManager.service
systemctl --root="$root" enable auditd.service firewalld.service chronyd.service \
    rsyslog.service usbguard.service fapolicyd.service
if [[ -e $root/usr/lib/systemd/system/lightdm.service ]]; then
    systemctl --root="$root" enable lightdm.service
    systemctl --root="$root" set-default graphical.target
    sed -i -E 's/^[[:blank:]]*autologin-user[[:blank:]]*=.*/# autologin disabled/' "$root/etc/lightdm/lightdm.conf"
else
    systemctl --root="$root" set-default multi-user.target
    # The radio console and local audit logs can start before DHCP completes.
    systemctl --root="$root" disable NetworkManager-wait-online.service
    # Update the immutable OS by replacing the image.
    systemctl --root="$root" disable dnf-makecache.timer
    # This image uses local accounts, GRUB, and NetworkManager. Avoid
    # starting directory services, a DNS trust-anchor updater, or dump capture.
    systemctl --root="$root" disable sssd.service sssd-kcm.socket \
        nis-domainname.service unbound-anchor.timer systemd-boot-update.service \
        kdump.service systemd-pstore.service
fi
install -d -m 0755 "$root/etc/systemd/system/user.slice.d"
cat > "$root/etc/systemd/system/linkwise.slice" <<'EOF'
[Unit]
Description=Linkwise modem and mail services

[Slice]
CPUWeight=1000
IOWeight=800
EOF
cat > "$root/etc/systemd/system/user.slice.d/50-linkwise-live.conf" <<'EOF'
[Slice]
CPUWeight=500
IOWeight=400
EOF
for service in linkwise-modem linkwise-mail-imap linkwise-mail-smtp; do
    if [[ -f $root/usr/lib/systemd/system/$service.service ]]; then
        install -d -m 0755 "$root/etc/systemd/system/$service.service.d"
        drop_in="$root/etc/systemd/system/$service.service.d/50-linkwise-live.conf"
        printf '[Service]\nSlice=linkwise.slice\n' > "$drop_in"
        chmod 0644 "$drop_in"
    fi
done
chmod 0644 "$root/etc/systemd/system/linkwise.slice" \
    "$root/etc/systemd/system/user.slice.d/50-linkwise-live.conf"
sed -i -E 's/^AuditBackend=.*/AuditBackend=LinuxAudit/; s/^PresentDevicePolicy=.*/PresentDevicePolicy=allow/; s/^InsertedDevicePolicy=.*/InsertedDevicePolicy=block/' \
    "$root/etc/usbguard/usbguard-daemon.conf"
install -m 0600 /dev/null "$root/etc/usbguard/rules.conf"
printf '%s\n' 'deny perm=any all : all' > "$root/etc/fapolicyd/rules.d/99-deny-everything.rules"
chmod 0644 "$root/etc/fapolicyd/rules.d/99-deny-everything.rules"
inside fapolicyd-cli --file add /usr/share/linkwise-live/
inside fapolicyd-cli --file add /usr/sbin/linkwise-shred
inside fapolicyd-cli --file add /usr/libexec/linkwise-usb-identify
inside fapolicyd-cli --file add /usr/bin/linkwise-mutt
inside fagenrules
install -d -m 0755 "$root/etc/firewalld/zones"
cat > "$root/etc/firewalld/zones/linkwise.xml" <<'EOF'
<?xml version="1.0" encoding="utf-8"?>
<zone target="DROP">
  <short>Linkwise live</short>
</zone>
EOF
sed -i 's/^DefaultZone=.*/DefaultZone=linkwise/' "$root/etc/firewalld/firewalld.conf"

# Dracut mounts the unlocked ext4 volume at this path before switching root.
cat > "$root/etc/fstab" <<'EOF'
tmpfs /tmp tmpfs defaults,nodev,nosuid,noexec 0 0
tmpfs /var/tmp tmpfs defaults,nodev,nosuid,noexec 0 0
tmpfs /dev/shm tmpfs defaults,nodev,nosuid,noexec 0 0
/run/initramfs/live/state/home /home none bind,nodev,nosuid,noexec 0 0
/run/initramfs/live/state/var /var none bind,nodev,nosuid 0 0
/var/log /var/log none bind,nodev,nosuid,noexec,x-systemd.requires-mounts-for=/var 0 0
/var/log/audit /var/log/audit none bind,nodev,nosuid,noexec,x-systemd.requires-mounts-for=/var/log 0 0
EOF
modules="$root/usr/share/selinux/packages"
semodule -X 200 -i "$modules/linkwise_modem.pp" -i "$modules/linkwise_modem_ports.cil"
if [[ -f $modules/flatpak.pp.bz2 ]]; then
    semodule -X 200 -i "$modules/flatpak.pp.bz2"
fi
setfiles -F -c "${policies[0]}" -r "$root" -e "$root/dev" -e "$root/proc" \
    -e "$root/run" -e "$root/sys" "$contexts" "$root"
[[ $(stat -c %C "$root/usr/lib/systemd/systemd") == *:init_exec_t:* ]]
[[ $(stat -c %C "$root/usr/bin/login") == *:login_exec_t:* ]]
