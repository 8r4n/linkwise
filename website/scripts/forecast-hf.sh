#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo 'Usage: forecast-hf.sh OUTPUT.json' >&2
    exit 2
fi

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
workspace="$(mktemp -d)"
trap 'rm -rf -- "$workspace"' EXIT
revision="$(PYTHONPATH="$root/tools/hf-live" python3 -c 'from path_forecast import REVISION; print(REVISION)')"
month="$(date -u +%m)"
upstream="$workspace/itu"
git init -q "$upstream"
git -C "$upstream" remote add origin https://github.com/ITU-R-Study-Group-3/ITU-R-HF.git
git -C "$upstream" sparse-checkout set --no-cone --stdin <<EOF
/P372/Linux/Makefile
/P372/Src/**/*.c
/P372/Src/**/*.h
/P533/Linux/Makefile
/P533/Src/**/*.c
/P533/Src/**/*.h
/ITURHFProp/Linux/Makefile
/ITURHFProp/Src/**/*.c
/ITURHFProp/Src/**/*.h
/P372/Data/COEFF${month}W.txt
/P372/Data/ionos${month}.bin
/P372/Data/P1239-3 Decile Factors.txt
EOF
git -C "$upstream" fetch -q --depth=1 --filter=blob:none origin "$revision"
git -C "$upstream" checkout -q --detach FETCH_HEAD
for module in P372 P533 ITURHFProp; do
    make -C "$upstream/$module/Linux" -j2
done

monthly="$workspace/solar-cycle.json"
curl --fail --silent --show-error --max-time 25 --retry 1 \
    https://services.swpc.noaa.gov/json/solar-cycle/predicted-solar-cycle.json -o "$monthly"
python3 "$root/tools/hf-live/path_forecast.py" --itu-root "$upstream" \
    --start-utc "$(date -u +%Y)-$month-01T00:00:00Z" --stations 48 --jobs 2 \
    --noaa-solar-cycle "$monthly" --output "$1"
