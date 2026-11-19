"""Exercise independent station processes, relay failover, and return receipts."""
import argparse
import contextlib
import hashlib
import json
import secrets
import socket
import subprocess
import tempfile
import time
from pathlib import Path


def exercise(binary, directory):
    def run(*args):
        return subprocess.check_output([binary, *args])

    nodes = "ABCDEF"
    # B is commissioned but stays offline. The surviving route is A-D-E-F.
    links = ["AB", "BC", "CF", "AD", "DE", "EF"]
    keys = {link: secrets.token_hex(32) for link in links}
    with contextlib.ExitStack() as ports:
        reservations = [ports.enter_context(socket.socket()) for _ in nodes]
        for sock in reservations:
            sock.bind(("127.0.0.1", 0))
        addresses = dict(zip(nodes, [f"127.0.0.1:{s.getsockname()[1]}" for s in reservations]))
    data = bytes(range(256)) * 17
    source = directory / "input.bin"
    source.write_bytes(data)
    ident = run("submit", directory / "A.db", "A", "F", source, "300").decode().strip()
    with contextlib.ExitStack() as stack:
        processes = []
        for slot, name in enumerate(nodes):
            if name == "B":
                continue
            peers = []
            for link in links:
                if name in link:
                    peer = link.replace(name, "")
                    peers.append(dict(name=peer, address=addresses[peer], key=keys[link]))
            profile = dict(listen=addresses[name], slot=slot, slots=6, slot_ms=125, peers=peers)
            config = directory / f"{name}.json"
            config.write_text(json.dumps(profile))
            log = stack.enter_context((directory / f"{name}.log").open("w"))
            proc = subprocess.Popen([binary, "serve", directory / f"{name}.db", name,
                                     config, "17"], stdout=log, stderr=log)
            stack.callback(stop, proc)
            processes.append(proc)
        for proc in processes:
            assert proc.wait(timeout=25) == 0
    assert run("read", directory / "F.db", "F", ident) == data
    rows = json.loads(run("list", directory / "A.db", "A"))
    assert next(r for r in rows if r["id"] == ident)["confirmed"]
    print(json.dumps(dict(nodes=6, offline="B", path="A-D-E-F", size=len(data),
                          sha256=hashlib.sha256(data).hexdigest(), source_confirmed=True)))


def stop(proc):
    if proc.poll() is None:
        proc.terminate()
    proc.wait(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory() as t:
        exercise(args.binary.resolve(), Path(t))
