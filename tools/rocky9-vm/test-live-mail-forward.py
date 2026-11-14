#!/usr/bin/env python3
"""Forward one pending fixture through virtual BW500 for live mail-unit testing."""
import hashlib
import os
from contextlib import ExitStack
from pathlib import Path
import runpy
import socket
import subprocess
import sys
import threading
import time

bench, profile, mailbox, peer = map(Path, sys.argv[1:])
helpers = runpy.run_path(str(bench))
with ExitStack() as stack:
    ports = [helpers["BENCH"]["start_station"](stack, "/usr/bin/linkwise-modemd", profile,
                                                call, 500)
             for call in helpers["CALLS"]]
    audio = [stack.enter_context(socket.create_connection(("127.0.0.1", port[2]))) for port in ports]
    for source, target in (audio, audio[::-1]):
        threading.Thread(target=helpers["relay"], args=(source, target), daemon=True).start()
    print("BENCH READY", flush=True)
    for _ in range(150):
        pending = list((mailbox / "outbox").glob("*.b2f"))
        if pending:
            break
        time.sleep(1)
    else:
        raise TimeoutError("No pending submission in the live SMTP mailbox")
    message, = pending
    processes = []
    for command, root, index in (("listen", peer, 1), ("exchange", mailbox, 0)):
        process = subprocess.Popen(["/usr/bin/linkwise", "mail", command, root,
                                    helpers["CALLS"][index], helpers["CALLS"][1-index],
                                    *map(str, ports[index][:2])])
        stack.callback(helpers["stop"], process)
        processes.append(process)
        time.sleep(0.3)
    assert all(process.wait(timeout=100) == 0 for process in reversed(processes))
    received = peer / "inbox" / message.name
    fixture = bytes((index * 31 + 7) % 256 for index in range(1400))
    assert received.read_bytes().endswith(fixture + b"\r\n")
    assert (mailbox / "finished" / message.name).read_text() == "confirmed\n"
    assert not message.exists()
    # The bench makes the peer inbox visible through the live IMAP unit.
    os.link(received, mailbox / "inbox" / message.name)
    print(f"SHA-256 {hashlib.sha256(fixture).hexdigest()} and clean disconnect both ends")
