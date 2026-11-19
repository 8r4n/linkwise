"""Run one LWN1 transfer over two actual virtual BW500 modem service engines."""
import argparse
import contextlib
import hashlib
import importlib.util
import json
import socket
import subprocess
import tempfile
from pathlib import Path
from threading import Thread
from time import monotonic

helper = Path(__file__).resolve().parents[3] / "tools/test-virtual-bw500.py"
spec = importlib.util.spec_from_file_location("fixture", helper)
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)


def pump(a, b):
    try:
        while chunk := a.recv(65536):
            b.sendall(chunk)
    except OSError:
        pass


def proxy(stack, data):
    listener = stack.enter_context(socket.socket())
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)

    def serve():
        peer, _ = listener.accept()
        with peer:
            Thread(target=pump, args=(data, peer), daemon=True).start()
            pump(peer, data)

    Thread(target=serve, daemon=True).start()
    return f"127.0.0.1:{listener.getsockname()[1]}"


def main(binary, modem, profile, root):
    payload = bytes(range(32))
    file = root / "payload"
    file.write_bytes(payload)
    db1, db2 = root / "A", root / "B"
    ident = subprocess.check_output([binary, "submit", db1, "A", "B", file, "600"]).decode().strip()
    with contextlib.ExitStack() as stack:
        a = fixture.attach(stack, modem, profile, "KN6MQS", 500)
        b = fixture.attach(stack, modem, profile, "0000", 500)
        start = monotonic()
        fixture.establish_bw500(a, b, peer_call="0000")
        connect_s = monotonic() - start
        for sock in (*a, *b):
            sock.settimeout(None)
        for x, y in [(a[2], b[2]), (b[2], a[2])]:
            Thread(target=pump, args=(x, y), daemon=True).start()
        recv = stack.enter_context(subprocess.Popen([binary,"receive-stream",db2,"B",proxy(stack,b[1])],stdout=subprocess.PIPE))
        stack.callback(lambda: recv.kill() if recv.poll() is None else None)
        start = monotonic()
        subprocess.run([binary,"send",db1,"A",ident,proxy(stack,a[1])],check=True,timeout=130)
        recv.communicate(timeout=10)
        assert recv.returncode==0
        assert subprocess.check_output([binary,"read",db2,"B",ident])==payload
        print(json.dumps(dict(connect_s=connect_s,transfer_s=monotonic()-start,
            payload_bytes=len(payload),payload_sha256=hashlib.sha256(payload).hexdigest(),
            modem_sha256=hashlib.sha256(modem.read_bytes()).hexdigest(),
            profile_sha256=hashlib.sha256(profile.read_bytes()).hexdigest(),paced=False)))


if __name__=="__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ("binary","modem","profile"):
        parser.add_argument(name,type=Path)
    a=parser.parse_args()
    with tempfile.TemporaryDirectory() as t:
        main(a.binary.resolve(),a.modem.resolve(),a.profile.resolve(),Path(t))
