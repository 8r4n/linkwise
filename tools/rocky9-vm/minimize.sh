#!/bin/bash
set -euo pipefail
. /etc/os-release
[[ $ID == rocky && $VERSION_ID == 9.* ]]
# Keep explicitly installed security and assessment packages. Suppress recursive removal.
dnf -y --setopt=clean_requirements_on_remove=False remove bluez udisks2 sssd-kcm kexec-tools
dnf clean packages
rpm -qa | sort
