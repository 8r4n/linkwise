#!/bin/bash
set -euo pipefail
stage=${1:?guest evidence stage}
rpm=$(find "$stage" -maxdepth 1 -name '*.rpm' -print -quit)
test -n "$rpm"
test_dir=$(mktemp -d /var/tmp/lw-evaluator-rpm.XXXXXX)
mkdir "$test_dir"/{empty,trusted}
rpmkeys --dbpath "$test_dir/trusted" --import "$stage/bench-public.asc"
rpmkeys --dbpath "$test_dir/trusted" --checksig "$rpm"
if rpmkeys --dbpath "$test_dir/empty" --checksig "$rpm"; then
    exit 1
fi
python3 - "$rpm" "$test_dir/modified.rpm" <<'PY'
from pathlib import Path
import sys
data = bytearray(Path(sys.argv[1]).read_bytes())
data[-10] ^= 1
Path(sys.argv[2]).write_bytes(data)
PY
if rpmkeys --dbpath "$test_dir/trusted" --checksig "$test_dir/modified.rpm"; then
    exit 1
fi
echo 'Signature controls passed in isolated Rocky RPM databases'
