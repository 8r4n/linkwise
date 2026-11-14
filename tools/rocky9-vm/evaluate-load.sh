#!/bin/bash
# Isolated audio and status stress on the installed service.
set -euo pipefail
cd "${1:?guest assessment directory}"
export LC_ALL=C
date -u
python3 - <<'PY'
import json, random, socket, subprocess, time
from pathlib import Path

unit = "linkwise-modem.service"
group = Path("/sys/fs/cgroup/system.slice", unit)
def read_status():
    with socket.create_connection(("127.0.0.1", 8300), timeout=2) as conn:
        with conn.makefile("rb") as response:
            return json.loads(response.readline(4096))
def property_value(name):
    return subprocess.check_output(["systemctl", "show", unit, "-p", name, "--value"]).strip()
initial_pid = property_value("MainPID")
initial_restarts = property_value("NRestarts")
assert initial_pid != b"0"
assert read_status()["ready"]
duration = 45
rng = random.Random(9341)
noise = rng.randbytes(48000 * 2 * duration)
with subprocess.Popen(
    ["aplay", "-q", "-t", "raw", "-f", "S16_LE", "-c", "1", "-r", "48000",
     "-D", "plughw:CARD=Loopback,DEV=1"], stdin=subprocess.PIPE,
) as player:
    import threading
    writer = threading.Thread(target=player.communicate, args=(noise,), daemon=True)
    writer.start()
    start = time.monotonic()
    deadline = start + duration + 15
    peak = 0
    queries = 0
    try:
        while writer.is_alive():
            assert time.monotonic() < deadline, "Playback deadline"
            assert read_status()["ready"]
            queries += 1
            peak = max(peak, int((group / "memory.current").read_text()))
            time.sleep(0.05)
    finally:
        if writer.is_alive():
            player.kill()
        writer.join()
assert player.returncode == 0
assert time.monotonic() - start >= duration - 2
assert property_value("MainPID") == initial_pid
assert property_value("NRestarts") == initial_restarts
assert peak <= 128 * 1024 * 1024, peak
print(f"PCM noise playback seconds={time.monotonic()-start:.2f} status_queries={queries} peak_sampled_bytes={peak}")
print("memory.max", (group / "memory.max").read_text().strip())
print("cpu.max", (group / "cpu.max").read_text().strip())
PY
python3 ./test-station-service.py ./bw500-level1-clean.wav --output plughw:CARD=Loopback,DEV=1 --level 1 --playback-timeout 30
