#!/bin/bash
# Exercise the installed receive-only service with a loopback-only network.
set -euo pipefail
cd "${1:?guest assessment directory}"
unit=linkwise-modem.service
dropin="/run/systemd/system/$unit.d/95-hf-isolation-test.conf"
test ! -e "$dropin"
install -d -m 0755 "$(dirname "$dropin")"
restore() {
    unlink "$dropin"
    systemctl daemon-reload
    systemctl restart "$unit"
}
trap restore EXIT
printf '[Service]\nPrivateNetwork=yes\n' > "$dropin"
systemctl daemon-reload
systemctl restart "$unit"
pid=$(systemctl show "$unit" -p MainPID --value)
test "$pid" -gt 0
nsenter -t "$pid" -n ip -br link
test "$(nsenter -t "$pid" -n ip -o link show | awk -F ': ' '{print $2}')" = lo
test -z "$(nsenter -t "$pid" -n ip route show default)"
test -z "$(nsenter -t "$pid" -n ip -6 route show default)"
nsenter -t "$pid" -n ss -ltnp
nsenter -t "$pid" -n python3 - <<'PY'
import errno, socket
for address, port, expected in (
    ("127.0.0.1", 8301, errno.ECONNREFUSED),
    ("192.0.2.1", 53, errno.ENETUNREACH),
    ("192.0.2.1", 25, errno.ENETUNREACH),
    ("192.0.2.1", 6514, errno.ENETUNREACH),
):
    try:
        with socket.create_connection((address, port), timeout=2):
            raise SystemExit(f"Unexpected connection to {address}:{port}")
    except OSError as error:
        assert error.errno == expected, error
        print(f"{address}:{port} {error.strerror}")
PY
nsenter -t "$pid" -n python3 ./test-station-service.py ./bw500-level1-clean.wav \
    --output plughw:CARD=Loopback,DEV=1 --level 1 --playback-timeout 30
