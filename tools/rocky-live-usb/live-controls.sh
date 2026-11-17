#!/usr/bin/env bash
# Run as root in the booted, encrypted USB guest. Evidence goes to encrypted /var.
set -euo pipefail
umask 077
evidence=${1:-/var/lib/linkwise-live-assessment}
mkdir -p "$evidence"
exec > >(tee "$evidence/controls.log") 2>&1
cat /proc/cmdline
kernel_fips=$(cat /proc/sys/crypto/fips_enabled)
crypto_policy=$(update-crypto-policies --show)
printf 'Kernel FIPS flag: %s\nSystem crypto policy: %s\n' "$kernel_fips" "$crypto_policy"
test "$kernel_fips" = 1
test "$crypto_policy" = FIPS
fips-mode-setup --check
openssl list -providers
openssl list -providers | grep -qx '  fips'
openssl dgst -sha256 /etc/os-release
if openssl dgst -md5 /etc/os-release >/dev/null 2>&1; then
    echo "OpenSSL accepted MD5 under the default crypto policy." >&2
    exit 1
fi
test "$(getenforce)" = Enforcing
getenforce
if [[ $(uname -m) == x86_64 ]]; then
    if ! dmesg | grep -E 'AMD Memory Encryption Features active:.*(SME|SEV)|x86/tme: enabled by BIOS'; then
        echo "Active x86 memory encryption was not reported by the kernel."
    fi
else
    echo "This architecture has no memory-encryption status check in Linkwise."
fi
findmnt -n -o FSTYPE / | grep -x overlay
findmnt -n -o FSTYPE /run/rootfsbase | grep -x squashfs

backing=$(findmnt -n -o SOURCE /run/initramfs/live)
device=$(cryptsetup status "$backing" | awk '$1 == "device:" {print $2}')
cryptsetup luksDump "$device" | grep -E '^(Version:|[[:space:]]+PBKDF:|[[:space:]]+cipher:)'
test "$(cryptsetup luksDump "$device" | awk '$1 == "Version:" {print $2}')" = 2
cryptsetup luksDump "$device" | grep -Eq '^[[:space:]]+PBKDF:[[:space:]]+pbkdf2$'
for mount in /home /var /var/log /var/log/audit; do
    source=$(findmnt -n -o SOURCE --mountpoint "$mount")
    lsblk -snr -o TYPE "${source%%\[*}" | grep -qx crypt
    printf 'LUKS-backed mount %s\n' "$mount"
done
test -z "$(swapon --show=NAME --noheadings)"
test "$(systemctl is-active auditd usbguard fapolicyd | sort -u)" = active
auditctl -s | tee "$evidence/audit-status.txt" | grep -qx 'enabled 2'
rpm -q rocky-release kernel-core openssl-fips-provider cryptsetup selinux-policy-targeted scap-security-guide

scan=0
oscap xccdf eval --profile xccdf_org.ssgproject.content_profile_stig \
    --results "$evidence/rocky-stig.xml" --report "$evidence/rocky-stig.html" \
    /usr/share/xml/scap/ssg/content/ssg-rl9-ds.xml > "$evidence/rocky-stig.log" 2>&1 || scan=$?
printf 'OpenSCAP exit %s\n' "$scan"
test "$scan" = 0 || test "$scan" = 2
python3 - "$evidence/rocky-stig.xml" <<'PY'
from collections import Counter
from sys import argv
import xml.etree.ElementTree as ET

root = ET.parse(argv[1]).getroot()
results = Counter(entry.text for entry in root.findall(".//{*}rule-result/{*}result"))
if not results:
    raise SystemExit("OpenSCAP report contains no rule results")
for outcome, count in sorted(results.items()):
    print(f"STIG {outcome}: {count}")
PY
