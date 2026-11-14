#!/bin/bash
# Evaluator tests for the isolated Rocky bench. Briefly stops the modem service.
set -euo pipefail
cd "${1:?guest evidence stage}"
unit=linkwise-modem.service
binary=/usr/bin/linkwise-modemd
config=/etc/linkwise/station.json
date -u
rpm -q linkwise-modem rocky-release kernel-core selinux-policy-targeted
sha256sum ./*.rpm "$binary" "$config" /usr/lib/systemd/system/"$unit"
rpmkeys --checksig ./*.rpm
rpm -V linkwise-modem
systemctl is-active "$unit"
getenforce
findmnt -no OPTIONS /
export TEST_DIR
TEST_DIR=$(mktemp -d /var/tmp/lw-evaluator.XXXXXX)
deny_config=/etc/linkwise-evaluator-denial.json
test ! -e "$deny_config"
cleanup() {
    unlink "$deny_config" 2>/dev/null || true
    systemctl start "$unit"
}
trap cleanup EXIT
export BIN="$binary" CONFIG="$config"
python3 - <<'PY'
from pathlib import Path
import json, os, subprocess
config = json.loads(Path(os.environ["CONFIG"]).read_bytes())
malformed = {
    "truncated": b'{"version":',
    "oversize": b" " * (16 * 1024 + 1),
    "unknown field": json.dumps(dict(config, unsupported=True)).encode(),
    "duplicate ports": json.dumps(dict(config, command_port=8301, data_port=8301)).encode(),
}
for index, (label, content) in enumerate(malformed.items()):
    path = Path(os.environ["TEST_DIR"]) / f"case-{index}.json"
    path.write_bytes(content)
    result = subprocess.run([os.environ["BIN"], "--check-config", str(path)],
                            capture_output=True, text=True, timeout=5)
    assert result.returncode == 1, (label, result.returncode, result.stderr)
    print("Rejected", label, result.stderr.strip())
PY
install -m 0644 "$config" "$deny_config"
chcon -t shadow_t "$deny_config"
stat -c '%C %n' "$deny_config"
tag="linkwise-evaluator-denial-$(date +%s)"
if systemd-run --quiet --wait --unit="$tag" -p DynamicUser=yes -p NoNewPrivileges=yes \
        "$binary" --serve-station "$deny_config"; then
    echo "Unexpected access to alternate-labeled station configuration"
    exit 1
fi
journalctl -u "$tag" --no-pager
grep -F "linkwise-evaluator-denial.json" /var/log/audit/audit.log | grep "type=AVC.*scontext=.*:linkwise_modem_t:.*tcontext=.*:shadow_t:.*permissive=0" | tail -1
python3 - <<'PY'
import concurrent.futures, errno, json, socket, time
start = time.monotonic()
def check(index):
    with socket.create_connection(("127.0.0.1", 8300), timeout=3) as conn:
        if index % 2 == 0:
            conn.sendall(b"CONNECT W1AW\r\x00" * 8)
        with conn.makefile("rb") as reply:
            status = json.loads(reply.readline(4096))
        assert status["ready"] is True, status
        assert set(status) == {"ready", "received_frames", "last_level"}, status
with concurrent.futures.ThreadPoolExecutor(max_workers=12) as pool:
    list(pool.map(check, range(600)))
print(f"600 local queries and command injection attempts completed in {time.monotonic()-start:.3f}s")
for addr, port in [("127.0.0.1", 8301), ("127.0.0.1", 1025), ("127.0.0.1", 1143)]:
    try:
        with socket.create_connection((addr, port), timeout=3):
            raise RuntimeError(f"Unexpected listener on {port}")
    except OSError as error:
        assert error.errno == errno.ECONNREFUSED, error
PY
systemctl show "$unit" -p MemoryCurrent -p NRestarts -p MainPID
start=$SECONDS
systemctl stop "$unit"
[[ $(systemctl show "$unit" -p Result --value) == success ]]
[[ $(systemctl show "$unit" -p MainPID --value) == 0 ]]
printf 'Clean stop in %s seconds\n' "$((SECONDS - start))"
systemctl start "$unit"
python3 ./test-station-service.py ./bw500-level1-clean.wav --output plughw:CARD=Loopback,DEV=1 --level 1 --playback-timeout 30
