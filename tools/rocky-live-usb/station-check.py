#!/usr/bin/env python3
"""Inventory live-session hardware and optionally check a Linkwise station."""
import argparse
import os
from pathlib import Path
import subprocess


def run_check(*args):
    try:
        return subprocess.run(args, timeout=15).returncode == 0
    except (OSError, subprocess.TimeoutExpired) as error:
        print(f"Could not complete {args[0]}: {error}", flush=True)
        return False


def serial_adapters():
    devices = {}
    for entry in sorted(Path("/dev/serial/by-id").glob("*")):
        if entry.exists():
            devices[entry.resolve()] = entry
    for pattern in ("ttyUSB*", "ttyACM*"):
        for device in sorted(Path("/dev").glob(pattern)):
            devices.setdefault(device, device)
    print("USB serial adapters (possible CAT or PTT interfaces):", flush=True)
    if not devices:
        print("  None detected. Connect the interface and run this check again.", flush=True)
    for device in devices.values():
        access = "accessible" if os.access(device, os.R_OK | os.W_OK) else "permission required"
        print(f"  {device} ({access})", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("station", nargs="?", type=Path, help="station.json to validate")
    args = parser.parse_args()

    print("Linkwise station check", flush=True)
    print("Audio devices and serial permissions are inspected without transmitting.", flush=True)
    audio_ok = run_check("linkwise-modemd", "--audio-devices")
    serial_adapters()
    if args.station is None:
        print("To check the selected audio channels and modem ports, pass station.json to this command.", flush=True)
    else:
        return 0 if run_check("linkwise-modemd", "--check-station", str(args.station)) and audio_ok else 1
    print("Choose Linkwise BW500 Offline Demo to try chat and mail over virtual audio.", flush=True)
    return 0 if audio_ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
