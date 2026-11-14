#!/bin/bash
set -euo pipefail
cd "${1:?assessment directory}"
suffix=${2:+-$2}
profile=xccdf_mil.disa.stig_profile_MAC-3_Sensitive
scan() {
    local label=$1
    shift
    local result=0
    oscap xccdf eval --profile "$profile" "$@" --results "$label.xml"         --report "$label.html" rhel9-v2r10-scap.xml > "$label.log" 2>&1 || result=$?
    printf '%s oscap exit=%s\n' "$label" "$result"
    [[ $result == 0 || $result == 2 ]]
}
status=0
scan "disa-native$suffix" || status=$?
rocky_ds=/usr/share/xml/scap/ssg/content/ssg-rl9-ds.xml
rocky_profile=xccdf_org.ssgproject.content_profile_stig
if [[ -r $rocky_ds ]]; then
    oscap info "$rocky_ds" > rocky-native-profiles.txt
    result=0
    oscap xccdf eval --profile "$rocky_profile" --results "rocky-native$suffix.xml" \
        --report "rocky-native$suffix.html" "$rocky_ds" > "rocky-native$suffix.log" 2>&1 || result=$?
    printf 'rocky-native%s oscap exit=%s\n' "$suffix" "$result"
    [[ $result == 0 || $result == 2 ]] || status=1
fi
exit "$status"
