#!/bin/bash
# Independent network stack on an isolated veth. No changes to UTM NAT.
set -euo pipefail
namespace=lw-eval-peer
link=lw-eval-host
ip netns add "$namespace"
cleanup() {
    ip link delete "$link" 2>/dev/null || true
    ip netns delete "$namespace"
}
trap cleanup EXIT
ip link add "$link" type veth peer name lw-eval-remote
ip link set lw-eval-remote netns "$namespace"
ip addr add 192.0.2.1/30 dev "$link"
ip link set "$link" up
ip -n "$namespace" addr add 192.0.2.2/30 dev lw-eval-remote
ip -n "$namespace" link set lw-eval-remote up
ip netns exec "$namespace" python3 - <<'PY'
import errno
import socket
from concurrent.futures import ThreadPoolExecutor

target = "192.0.2.1"
# Confirm the route and remote ingress path through the existing SSH service.
with socket.create_connection((target, 22), timeout=3) as stream:
    assert stream.recv(64).startswith(b"SSH-")
print("Independent network stack reached SSH positive control")
def blocked(port):
    try:
        with socket.create_connection((target, port), timeout=3):
            raise RuntimeError(f"Unexpected modem listener at {target}:{port}")
    except OSError as error:
        assert error.errno in (errno.ECONNREFUSED, errno.ETIMEDOUT, errno.EHOSTUNREACH, None), error
        print(f"Modem port {port} unreachable: {error}")
with ThreadPoolExecutor(max_workers=2) as pool:
    list(pool.map(blocked, (8300, 8301)))
PY
