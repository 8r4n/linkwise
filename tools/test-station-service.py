#!/usr/bin/env python3
"""Check a configured Linux station service with an isolated ALSA loopback."""

import argparse
import json
import socket
import subprocess
import time


def status(port):
    with socket.create_connection(("127.0.0.1", port), timeout=1) as connection:
        with connection.makefile("rb") as response:
            return json.loads(response.readline(4096))


def wait_for(port, predicate):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        try:
            current = status(port)
            if predicate(current):
                return current
        except OSError:
            pass
        time.sleep(0.2)
    raise RuntimeError("Station did not reach the expected state")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("waveform", help="48 kHz mono PCM WAV with a known BW500 DATA frame")
    parser.add_argument("--output", required=True, help="isolated ALSA loopback playback PCM")
    parser.add_argument("--port", type=int, default=8300)
    parser.add_argument("--level", type=int, required=True)
    args = parser.parse_args()

    # Play one continuous stream. Closing and reopening the writer can
    # renegotiate the loopback PCM format while the receiver is still open.
    initial = wait_for(args.port, lambda current: "received_frames" in current)
    subprocess.run(["aplay", "-q", "-D", args.output, args.waveform], check=True, timeout=15)
    result = wait_for(
        args.port,
        lambda current: current["ready"]
        and current["received_frames"] > initial["received_frames"]
        and current["last_level"] == args.level,
    )
    print(json.dumps(result))


if __name__ == "__main__":
    main()
