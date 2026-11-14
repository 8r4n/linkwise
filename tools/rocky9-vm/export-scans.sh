#!/bin/bash
set -euo pipefail
umask 077
cd "${1:?stage directory}"
shopt -s nullglob
files=(*-native*.{xml,html,log} rocky-native-profiles.txt *.{generated,selected}.sh)
tar -cf scans.tar -- "${files[@]}"
chown lwadmin: scans.tar
