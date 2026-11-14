#!/bin/bash
set -euo pipefail
. /etc/os-release
[[ $ID == rocky && $VERSION_ID == 9.* ]]
for setting in '!targetpw' '!rootpw' '!runaspw'; do
    grep -Fxq "Defaults $setting" /etc/sudoers ||
        printf 'Defaults %s\n' "$setting" >> /etc/sudoers
done
visudo -cf /etc/sudoers
printf 'typeset -xr TMOUT=600\n' > /etc/profile.d/99-linkwise-timeout.sh
for home in /root /home/lwadmin; do
    find "$home" -maxdepth 1 -type f -name '.*' -exec chmod u-s,g-wxs,o= {} +
done
for name in .bash_logout .bash_profile .bashrc .cshrc .tcshrc; do
    printf 'C /root/%s 600 root root - /usr/share/rootfiles/%s\n' "$name" "$name"
done > /etc/tmpfiles.d/rootfiles.conf
authselect enable-feature without-nullok
authselect check
restorecon /etc/sudoers /etc/tmpfiles.d/rootfiles.conf /etc/profile.d/99-linkwise-timeout.sh
