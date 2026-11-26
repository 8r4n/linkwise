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


def exercise(binary, directory, live_queue):
    def run(*args):
        return subprocess.check_output([binary, *args], timeout=10)

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
    control = {node: directory / f"{node}.sock" for node in "AF"} if live_queue else {}
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
            if name in control:
                profile["control_socket"] = str(control[name])
            config = directory / f"{name}.json"
            config.write_text(json.dumps(profile))
            log = stack.enter_context((directory / f"{name}.log").open("w"))
            proc = subprocess.Popen([binary, "serve", directory / f"{name}.db", name,
                                     config, "17"], stdout=log, stderr=log)
            stack.callback(stop, proc)
            processes.append(proc)
        if live_queue:
            wait_until(lambda: all(path.is_socket() for path in control.values()), 5)
            args = ("queue", control["A"], "submit", "live-fixture", "F", source, "300")
            live_id = run(*args).decode().strip()
            assert live_id != ident
            assert run(*args).decode().strip() == live_id
            conflict = subprocess.run([binary, *args[:-1], "301"], capture_output=True, timeout=10)
            assert conflict.returncode != 0 and b"already used for different data" in conflict.stderr

            def confirmed():
                rows = json.loads(run("queue", control["A"], "list"))
                return {row["id"] for row in rows if row["confirmed"]} >= {ident, live_id}

            wait_until(confirmed, 15)
            assert run("queue", control["F"], "read", live_id) == data
        for proc in processes:
            assert proc.wait(timeout=25) == 0
    assert run("read", directory / "F.db", "F", ident) == data
    rows = json.loads(run("list", directory / "A.db", "A"))
    assert next(r for r in rows if r["id"] == ident)["confirmed"]
    if live_queue:
        assert next(r for r in rows if r["id"] == live_id)["confirmed"]
        assert all(not path.exists() for path in control.values())
    print(json.dumps(dict(nodes=6, offline="B", path="A-D-E-F", size=len(data),
                          sha256=hashlib.sha256(data).hexdigest(), source_confirmed=True,
                          live_queue=live_queue)))


def wait_until(predicate, seconds):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.05)
    raise AssertionError("station fixture timed out")


def stop(proc):
    if proc.poll() is None:
        proc.terminate()
    proc.wait(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--live-queue", action="store_true")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(dir="/tmp" if args.live_queue else None) as t:
        try:
            exercise(args.binary.resolve(), Path(t), args.live_queue)
        except Exception:
            for log in sorted(Path(t).glob("*.log")):
                print(log.name, log.read_text())
            raise
