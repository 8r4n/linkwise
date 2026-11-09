#!/usr/bin/env python3
"""Fetch pinned reference audio, verify it, and run all Rust tests including replay."""

import argparse
import hashlib
import importlib.util
import os
from pathlib import Path
import subprocess
import urllib.request


ROOT = Path(__file__).resolve().parent.parent
REVISION = "952f79245240132f4af209afbad1f4b2ba256613"
SOURCE = (
    f"https://raw.githubusercontent.com/sstjohn/hfmodem/{REVISION}/"
    "packages/hfmodem/hfmodem/tests/kestrel/fixtures/bw500-low-levels/"
)
FILES = {
    "provenance.json": "002d03b14a3711430c30f9eecf3c73683664382a7ddb33ca61bfb5c52354e1a4",
    "bw500-level1-arm1.wav": "df1a22c065afccf01daa6e342590c1f7c251c197c5b09816c909aa0f6d69f311",
    "bw500-level1-arm2.wav": "e596bb2fc1a1b37be65e7f2e2ef95d1d8ef00ca1ed66af12a77a4dad202522a3",
    "bw500-level2-arm1.wav": "407cddd522840044094d9a47cd1a7a9752801ce1fb1f89ec9d981d4601c48ce3",
    "bw500-level2-arm2.wav": "990e7298553ac88a763debd8e9564a3ef074047405a2e1a0f2bec3f26d82a911",
    "bw500-level1-two-frames.wav": "49362d3b65662df303afd84932f2685d772c047985a6a7c209d876538613029f",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--audio-device", action="store_true", help="also test the optional CPAL input backend")
    args = parser.parse_args()
    destination = ROOT / "reference/vara/fixtures/bw500"
    destination.mkdir(parents=True, exist_ok=True)
    for name, expected in FILES.items():
        path = destination / name
        if path.exists():
            data = path.read_bytes()
        else:
            print(f"Downloading {name}", flush=True)
            with urllib.request.urlopen(SOURCE + name, timeout=60) as response:
                data = response.read()
        if hashlib.sha256(data).hexdigest() != expected:
            raise SystemExit(f"SHA-256 mismatch: {path}; refusing unverified audio")
        if not path.exists():
            path.write_bytes(data)
    spec = importlib.util.spec_from_file_location("modem_profile", ROOT / "tools/fetch-modem-profile.py")
    profile_tool = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(profile_tool)
    profile = profile_tool.fetch_profile()
    environment = dict(os.environ, LINKWISE_BW500_FIXTURES=str(destination), LINKWISE_BW500_PROFILE=str(profile))
    command = ["cargo", "test", "--locked"]
    if args.audio_device:
        command += ["--features", "audio-device"]
    subprocess.run(
        command + ["--", "--include-ignored"],
        cwd=ROOT / "rust", env=environment, check=True,
    )


if __name__ == "__main__":
    main()
