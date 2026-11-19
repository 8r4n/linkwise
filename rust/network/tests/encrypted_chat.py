"""Run simultaneous mutually authenticated encrypted chats on virtual BW500 links."""
import argparse
import concurrent.futures
import copy
import multilink
import json
import os
import runpy
import tempfile
from pathlib import Path


def exercise(pair, root, args):
    # Each independent harness owns its identities, CA, evidence and two modems.
    bench = runpy.run_path(str(args.tools / "test-pki.py"))
    stations = (f"T{2 * pair + 1:03}AA", f"T{2 * pair + 2:03}AA")
    directory = root / stations[0]
    directory.mkdir()
    bench["issue"].__globals__["CALLS"] = stations
    bench["issue"](directory)
    configs = []
    for station, peer in (stations, stations[::-1]):
        path = bench["profile"](directory, station, peer)
        profile = json.loads(path.read_text())
        profile["allow_chat_encryption"] = True
        path.write_text(json.dumps(profile))
        bench["run"](args.cli, "pki", "check", path)
        configs.append(path)
    args = copy.copy(args)
    if args.network:
        args.data_adapter = lambda stack, side, ports: multilink.attach(
            stack, args.network, directory, side, ports)
    bench["chat"](args, configs, encrypted=True)
    return stations


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("cli", "modem", "profile"):
        parser.add_argument(name)
    parser.add_argument("--tools", type=Path, default=Path(__file__).resolve().parents[3] / "tools")
    parser.add_argument("--pairs", type=int, default=4, choices=range(1, 33))
    parser.add_argument("--network", help="relay each outbound DATA stream over three BP hops")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory() as temp:
        os.environ["TMPDIR"] = temp
        with concurrent.futures.ThreadPoolExecutor(max_workers=args.pairs) as executor:
            jobs = [executor.submit(exercise, i, Path(temp), args) for i in range(args.pairs)]
            print(json.dumps({"encrypted_pairs": [job.result() for job in jobs]}))
