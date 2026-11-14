#!/usr/bin/env python3
"""Run the isolated Linkwise side of the Thunderbird iOS simulator test."""

import argparse
from contextlib import ExitStack
import hashlib
import imaplib
import os
from pathlib import Path
import runpy
import smtplib
import socket
import ssl
import subprocess
import threading
import time

REPO = Path(__file__).resolve().parents[2]
BENCH = runpy.run_path(str(REPO / "tools/test-winlink-modem.py"))
CALLER, PEER = BENCH["CALLS"]
EXPECTED = bytes((index * 31 + 7) % 256 for index in range(1400))
SMTP_PORT, IMAP_PORT = 12525, 11143
FIXTURE_ENV = {
    "LINKWISE_SMTP_USER": "bench",
    "LINKWISE_SMTP_PASSWORD": "fixture",
    "LINKWISE_IMAP_USER": "bench",
    "LINKWISE_IMAP_PASSWORD": "fixture",
}


def check_rejections(tls, mailbox):
    trust = ssl.create_default_context(cafile=str(tls / "ca.pem"))
    with smtplib.SMTP("localhost", SMTP_PORT, timeout=5) as sender:
        assert sender.ehlo()[0] == 250
        assert sender.docmd("AUTH", "LOGIN")[0] == 538
        assert sender.mail(f"{CALLER}@winlink.org")[0] == 530
        sender.starttls(context=trust)
        sender.ehlo()
        try:
            sender.login("bench", "wrong")
        except smtplib.SMTPAuthenticationError as error:
            assert error.smtp_code == 535
        else:
            raise AssertionError("SMTP accepted a bad password")
        assert sender.mail(f"{CALLER}@winlink.org")[0] == 530
        assert sender.login("bench", "fixture")[0] == 235
        assert sender.mail(f"{PEER}@winlink.org")[0] == 553
        assert sender.mail(f"{CALLER}@winlink.org")[0] == 250
        assert sender.rcpt(f"{PEER}@winlink.org")[0] == 250
        assert sender.docmd("DATA")[0] == 354
        sender.send(b"Subject: Interrupted\r\n\r\nPartial attachment\r\n")
        sender.close()
    with smtplib.SMTP("localhost", SMTP_PORT, timeout=5) as sender:
        sender.starttls(context=trust)
        sender.login("bench", "fixture")
        assert sender.mail(f"{CALLER}@winlink.org")[0] == 250
        assert sender.rcpt(f"{PEER}@winlink.org")[0] == 250
        assert sender.docmd("DATA")[0] == 354
        sender.send((b"x" * 990 + b"\r\n") * 2115)
        assert sender.getreply()[0] == 552
        sender.close()
    assert not list((mailbox / "outbox").iterdir())
    with imaplib.IMAP4_SSL("localhost", IMAP_PORT, ssl_context=trust, timeout=5) as reader:
        try:
            reader.login("bench", "wrong")
        except imaplib.IMAP4.error:
            pass
        else:
            raise AssertionError("IMAP accepted a bad password")
        assert reader.login("bench", "fixture")[0] == "OK"
    for host, context in (("localhost", ssl.create_default_context()), ("127.0.0.1", trust)):
        try:
            with imaplib.IMAP4_SSL(host, IMAP_PORT, ssl_context=context, timeout=5):
                pass
        except ssl.SSLCertVerificationError:
            pass
        else:
            raise AssertionError(f"Unexpected certificate acceptance for {host}")
    print("Verified authentication, sender restriction, interrupted and oversized DATA, and certificate rejection", flush=True)


def relay_hf(source, target, worker):
    try:
        while True:
            frame = BENCH["BENCH"]["waveform"](source)
            worker.stdin.write(frame)
            worker.stdin.flush()
            filtered = worker.stdout.read(len(frame))
            if len(filtered) != len(frame):
                raise OSError("GNU Radio worker returned a short PCM record")
            target.sendall(filtered)
    except (OSError, EOFError) as error:
        print(f"HF relay ended: {error}", flush=True)


def run(work, tls, hf_worker=None, snr=None, seed=17, preflight_only=False):
    work.mkdir()
    binaries = REPO / "rust/target/debug"
    profile = REPO / "reference/vara/fixtures/level4/bw500-level4.json"
    with ExitStack() as stack:
        ports = [BENCH["BENCH"]["start_station"](stack, binaries / "linkwise-modemd", profile, call, 500)
                 for call in (CALLER, PEER)]
        audio = [stack.enter_context(socket.create_connection(("127.0.0.1", port[2]))) for port in ports]
        for source, target in (audio, audio[::-1]):
            if hf_worker is None:
                threading.Thread(target=BENCH["relay"], args=(source, target), daemon=True).start()
                continue
            delay, spread, echo = (1, 0.5, -9) if snr == 32 else (2, 1, -4)
            worker = subprocess.Popen([
                "docker", "run", "--rm", "-i", "-e", "BENCH_RICIAN_K=10",
                "-v", f"{hf_worker.resolve()}:/channel.py:ro", "linkwise-hf-bench",
                "python3", "-u", "/channel.py", *map(str, (snr, delay, spread, echo, seed)),
            ], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
            stack.callback(BENCH["stop"], worker)
            threading.Thread(target=relay_hf, args=(source, target, worker), daemon=True).start()

        def spawn(*arguments):
            process = subprocess.Popen([binaries / "linkwise", "mail", *map(str, arguments)],
                                       env=os.environ | FIXTURE_ENV)
            stack.callback(BENCH["stop"], process)
            return process

        sender = spawn("smtp", work / CALLER, CALLER, SMTP_PORT, "--starttls", tls / "server.pem", tls / "server.key")
        reader = spawn("imap", work / PEER, IMAP_PORT, "--tls", tls / "server.pem", tls / "server.key")
        for process, port in ((sender, SMTP_PORT), (reader, IMAP_PORT)):
            for _ in range(50):
                if process.poll() is not None:
                    raise RuntimeError(f"Mail listener on {port} exited")
                try:
                    with socket.create_connection(("127.0.0.1", port), timeout=1):
                        break
                except ConnectionRefusedError:
                    time.sleep(0.1)
            else:
                raise TimeoutError(f"Mail listener on {port} did not start")
        check_rejections(tls, work / CALLER)
        if preflight_only:
            return
        print("BENCH READY", flush=True)
        for _ in range(240):
            queued = list((work / CALLER / "outbox").glob("*.b2f"))
            if queued:
                break
            time.sleep(1)
        else:
            raise TimeoutError("No simulator submission")
        message, = queued
        peer = spawn("listen", work / PEER, PEER, CALLER, *ports[1][:2])
        time.sleep(0.3)
        caller = spawn("exchange", work / CALLER, CALLER, PEER, *ports[0][:2])
        caller_exit, peer_exit = caller.wait(timeout=100), peer.wait(timeout=100)
        if (caller_exit, peer_exit) != (0, 0):
            raise RuntimeError(f"BW500 exchange failed: caller={caller_exit}, responder={peer_exit}")
        received, = (work / PEER / "inbox").glob("*.b2f")
        assert received.name == message.name
        assert received.read_bytes()[-len(EXPECTED) - 2:-2] == EXPECTED
        assert (work / CALLER / "finished" / message.name).read_text() == "confirmed\n"
        assert not message.exists()
        print(f"BW500 SHA-256 {hashlib.sha256(EXPECTED).hexdigest()} and clean disconnect both ends", flush=True)
        time.sleep(30)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("work", type=Path, help="new directory for disposable mailboxes")
    parser.add_argument("tls", type=Path, help="directory containing ca.pem, server.pem and server.key")
    parser.add_argument("--hf-worker", type=Path, help="framed PCM GNU Radio channel worker")
    parser.add_argument("--snr", type=int, choices=(32, 22))
    parser.add_argument("--seed", type=int, default=17)
    parser.add_argument("--preflight-only", action="store_true")
    args = parser.parse_args()
    if bool(args.hf_worker) != bool(args.snr):
        parser.error("--hf-worker and --snr are used together")
    run(args.work, args.tls, args.hf_worker, args.snr, args.seed, args.preflight_only)
