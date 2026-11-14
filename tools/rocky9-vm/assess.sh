#!/bin/bash
# Stage test inputs, apply the baseline and capture pre/post results.
set -euo pipefail
umask 077
here=$(cd "$(dirname "$0")" && pwd)
repo=$(git -C "$here" rev-parse --show-toplevel)
work="$HOME/Virtual Machines/Linkwise Rocky9 Evidence"
remote=/home/lwadmin/rocky-assessment
ssh="$here/ssh.sh"
signing="$work/build-source/signing-bench"
rpm=$(find "$signing" -maxdepth 1 -name '*.rpm' -print -quit)
test -n "$rpm"
root() {
    "$ssh" "sudo -S -p '' $*" < "$work/admin-password.txt"
}
if [[ ${1:-} != post-reboot ]]; then
    root 'dnf -y install tar' > "$work/guest-bootstrap.log" 2>&1
    "$ssh" "mkdir -p -m 0700 '$remote'"
    tar --no-xattrs -cf - -C "$here" lw-rocky-collect.sh lw-rocky-hardening.sh deploy-linkwise.sh scan-stig.sh apply-stig.sh \
        minimize.sh remediate-profile.py apply-accounts-audit.sh apply-login-audit.sh verify-login-audit.sh export-scans.sh \
        apply-local-policy.sh enable-application-control.sh test-isolation.sh \
        close-local-findings.sh close-firewall.sh apply-evaluation-limits.sh \
        evaluate-ingress.sh evaluate-load.sh review-platform.sh \
        evaluate-hf-isolation.sh close-user-namespaces.sh apply-grub-auth.sh \
        -C "$work" rhel9-v2r10-scap.xml -C "$signing" bench-public.asc "${rpm##*/}" \
        -C "$repo/tools" test-station-service.py verify-niap-linux.sh \
        -C "$repo/docs/media/modem" bw500-level1-clean.wav | "$ssh" "tar -xf - -C '$remote'"
    root "bash '$remote/lw-rocky-collect.sh' '$remote/before'" > "$work/guest-before.log" 2>&1
    root "bash '$remote/lw-rocky-hardening.sh'" > "$work/guest-upgrade.log" 2>&1
    root "bash '$remote/scan-stig.sh' '$remote' before" > "$work/guest-stig-before.log" 2>&1
    root "bash '$remote/deploy-linkwise.sh' '$remote'" > "$work/guest-linkwise.log" 2>&1
    root "bash '$remote/apply-stig.sh'" > "$work/guest-stig-remediation.log" 2>&1
    "$here/reboot.sh"
    root "bash '$remote/scan-stig.sh' '$remote' intermediate" > "$work/guest-stig-intermediate.log" 2>&1
    root "bash '$remote/minimize.sh'" > "$work/guest-minimize.log" 2>&1
    root "python3 '$remote/remediate-profile.py' '$remote/rocky-native-intermediate.xml' --apply" > "$work/guest-profile-remediation.log" 2>&1
    root "bash '$remote/apply-accounts-audit.sh'" > "$work/guest-accounts-audit.log" 2>&1
    "$here/reboot.sh"
    root "bash '$remote/enable-application-control.sh'" > "$work/guest-fapolicyd.log" 2>&1
    root "bash '$remote/apply-local-policy.sh'" > "$work/guest-local-policy.log" 2>&1
    root "bash '$remote/close-local-findings.sh'" > "$work/gate-local-controls.log" 2>&1
    root "bash '$remote/close-firewall.sh'" > "$work/gate-firewall.log" 2>&1
    root "bash '$remote/apply-evaluation-limits.sh'" > "$work/gate-resource-limits.log" 2>&1
    root "bash '$remote/close-user-namespaces.sh'" > "$work/hf-userns.log" 2>&1
    "$here/configure-grub-auth.sh" > "$work/hf-grub-config.log" 2>&1
    "$here/reboot.sh" check-grub
fi
root "bash '$remote/lw-rocky-collect.sh' '$remote/after'" > "$work/guest-after.log" 2>&1
root "bash '$remote/verify-niap-linux.sh' '$remote/${rpm##*/}'" > "$work/guest-linkwise-verifier.log" 2>&1
root "bash '$remote/scan-stig.sh' '$remote' after" > "$work/guest-stig-after.log" 2>&1
"$ssh" "rpm -qa | sort" > "$work/guest-rpm-inventory.txt"
root "bash '$remote/test-isolation.sh'" > "$work/guest-isolation.log" 2>&1
mkdir -p "$work/scans"
# Separate authentication from binary stdout. Use a private bundle owned by the SSH account.
root "bash '$remote/export-scans.sh' '$remote'"
"$ssh" "cat '$remote/scans.tar'" | tar -xf - -C "$work/scans"
root "python3 '$remote/test-station-service.py' '$remote/bw500-level1-clean.wav' --output plughw:CARD=Loopback,DEV=1 --level 1 --playback-timeout 30" > "$work/guest-audio.log" 2>&1
root "bash '$remote/review-platform.sh'" > "$work/gate-platform-review.log" 2>&1
root "bash '$remote/evaluate-ingress.sh'" > "$work/gate-ingress.log" 2>&1
root "bash '$remote/evaluate-load.sh' '$remote'" > "$work/gate-load.log" 2>&1
root "bash '$remote/verify-login-audit.sh'" > "$work/rocky-login-audit.log" 2>&1
root "bash '$remote/evaluate-hf-isolation.sh' '$remote'" > "$work/hf-isolation.log" 2>&1
