#!/usr/bin/env python3
"""Cache published level-4 measurements; extract literals without executing code."""
import ast
import hashlib
import json
from pathlib import Path
import urllib.request

REVISION = "952f79245240132f4af209afbad1f4b2ba256613"
BASE = f"https://raw.githubusercontent.com/sstjohn/hfmodem/{REVISION}/"


def literal(source, name):
    for node in ast.parse(source).body:
        if isinstance(node, ast.Assign) and any(
            isinstance(target, ast.Name) and target.id == name for target in node.targets
        ):
            return ast.literal_eval(node.value)
    raise ValueError(f"missing measurement {name}")


def fetch_profile():
    root = Path(__file__).resolve().parent.parent / "reference/vara/fixtures/level4"
    root.mkdir(parents=True, exist_ok=True)
    sources = {}
    for name, remote in {
        "tables.py": "packages/hfmodem/hfmodem/kestrel/rx/tablegen.py",
        "pulse.py": "packages/hfmodem/hfmodem/kestrel/tx/bw500_pulse.py",
    }.items():
        path = root / name
        if path.exists():
            data = path.read_bytes()
        else:
            with urllib.request.urlopen(BASE + remote, timeout=60) as response:
                data = response.read()
        expected = HASHES[name]
        if hashlib.sha256(data).hexdigest() != expected:
            raise ValueError(f"SHA-256 mismatch: {path}")
        if not path.exists():
            path.write_bytes(data)
        sources[name] = data.decode()
    profile = {
        "source": "Saul St John (W9SSJ), hfmodem published measurements",
        "revision": REVISION,
        "license": "AGPL-3.0-only; downloaded data, not redistributed by Linkwise",
        "grid": literal(sources["tables.py"], "_GRID480_CELLS"),
        "pulse": literal(sources["pulse.py"], "PULSE"),
    }
    path = root / "bw500-level4.json"
    path.write_text(json.dumps(profile, indent=2) + "\n")
    return path


HASHES = {
    "tables.py": "5560ea3bc3794c9864aa11a97513e0231e642856f9b0009d0f4080c556d30bf3",
    "pulse.py": "e85f421ec075cba7c5fcb820e9ba79318f699af4a85cb67961305aecfb09c042",
}

if __name__ == "__main__":
    print(fetch_profile())
