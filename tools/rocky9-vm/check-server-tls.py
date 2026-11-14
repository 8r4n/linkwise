#!/usr/bin/env python3
"""Verify Rocky OpenSSL policy enforcement before running client tests."""

import os
from pathlib import Path
import socket
import subprocess
import sys

binary, tls, stage = map(Path, sys.argv[1:])
environment = os.environ | {
    "LINKWISE_SMTP_USER": "bench",
    "LINKWISE_SMTP_PASSWORD": "fixture",
}
with socket.socket() as probe:
    probe.bind(("127.0.0.1", 0))
    port = probe.getsockname()[1]
command = [binary, "mail", "smtp", stage, "KN6MQS", str(port), "--starttls",
           tls / "server.pem", tls / "server.key"]
with_disabled_policy = subprocess.run(
    command, env=environment | {"OPENSSL_CONF": "/dev/null"}, timeout=10,
    capture_output=True, text=True,
)
assert with_disabled_policy.returncode != 0
assert "requires kernel FIPS mode and the system OpenSSL FIPS policy" in with_disabled_policy.stderr
with socket.socket() as probe:
    probe.settimeout(1)
    assert probe.connect_ex(("127.0.0.1", port)) != 0
print("SMTP refused startup and did not bind with OpenSSL policy disabled")
for protocol, args in (("smtp", ["KN6MQS"]), ("imap", [])):
    rejected = subprocess.run(
        [binary, "mail", protocol, stage, *args, str(port)],
        env=os.environ | {f"LINKWISE_{protocol.upper()}_BIND": "0.0.0.0"},
        timeout=10, capture_output=True, text=True,
    )
    assert rejected.returncode != 0
    assert "Non-loopback mail listeners require TLS and authentication" in rejected.stderr
    print(f"{protocol.upper()} rejected unauthenticated plaintext bind")
