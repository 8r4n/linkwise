#!/bin/bash
set -euo pipefail
work=${1:?"assessment evidence directory"}
base=https://dl.dod.cyber.mil/wp-content/uploads/stigs/zip
cd "$work"
for suffix in STIG STIG_SCAP_1-3_Benchmark STIG_Ansible; do
    name="U_RHEL_9_V2R10_$suffix.zip"
    curl -fsSL --retry 3 "$base/$name" -o "$name"
done
shasum -a 256 U_RHEL_9_V2R10_*.zip > disa-artifacts.sha256
unzip -p U_RHEL_9_V2R10_STIG_SCAP_1-3_Benchmark.zip '*.xml' > rhel9-v2r10-scap.xml
