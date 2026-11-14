#!/usr/bin/env python3
"""Exercise B2F mail and clean disconnect over two virtual BW500 modems."""

import argparse
import contextlib
import hashlib
import imaplib
import json
import os
from datetime import datetime, timezone
from email import policy
from email.message import EmailMessage
from email.parser import BytesParser
import runpy
import smtplib
import socket
import subprocess
import tempfile
import threading
import time
from pathlib import Path

TOOLS = Path(__file__).parent
BENCH = runpy.run_path(str(TOOLS / "test-virtual-bw500.py"))
CALLS = ("KN6MQS", "W1AW")


def relay(source, destination):
    try:
        while chunk := source.recv(65536):
            destination.sendall(chunk)
    except OSError:
        pass


def proxy(listener, target):
    with listener.accept()[0] as client, socket.create_connection(("127.0.0.1", target)) as modem:
        reverse = threading.Thread(target=relay, args=(modem, client), daemon=True)
        reverse.start()
        relay(client, modem)


def stop(process):
    if process.poll() is None:
        process.kill()
        process.wait()


def mail_client(stack, cli, command, mailbox, connect, *args):
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    process = subprocess.Popen([cli, "mail", command, mailbox, *args, str(port)])
    stack.callback(stop, process)
    for _ in range(50):
        if process.poll() is not None:
            raise RuntimeError(f"{command} listener exited with status {process.returncode}")
        try:
            return stack.enter_context(connect("127.0.0.1", port, timeout=5))
        except ConnectionRefusedError:
            time.sleep(0.05)
    raise TimeoutError(f"{command} listener did not start")


def test(args):
    with contextlib.ExitStack() as stack:
        root = Path(stack.enter_context(tempfile.TemporaryDirectory()))
        ports = [BENCH["start_station"](stack, args.modem, args.profile, call, 500)
                 for call in CALLS]
        pat_env = None
        if args.pat:
            # Pat uses adjacent local ports. Forward them to the modem's
            # independently allocated loopback sockets.
            command = socket.socket()
            stack.callback(command.close)
            data = socket.socket()
            stack.callback(data.close)
            for _ in range(32):
                command.bind(("127.0.0.1", 0))
                base = command.getsockname()[1]
                try:
                    data.bind(("127.0.0.1", base + 1))
                    break
                except OSError:
                    command.close()
                    command = socket.socket()
                    stack.callback(command.close)
            else:
                raise RuntimeError("could not allocate adjacent Pat ports")
            for endpoint, target in zip((command, data), ports[0]):
                endpoint.listen(1)
                threading.Thread(target=proxy, args=(endpoint, target), daemon=True).start()
            config = root / "pat.json"
            config.write_text(json.dumps({
                "mycall": CALLS[0], "locator": "AA00aa",
                "version_reporting_disabled": True,
                "varahf": {"addr": f"127.0.0.1:{base}", "bandwidth": 500},
            }))
            # Seed the disposable client cache so P2P never calls CMS
            # to check the bench callsign's account status.
            pat_env = dict(os.environ, XDG_STATE_HOME=str(root / "state"),
                           XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"))
            state = root / "state" / "pat"
            state.mkdir(parents=True)
            stamp = datetime.now(timezone.utc).isoformat()
            for key in ("account-confirmed", "pw_recovery_email_check"):
                (state / f".{key}_{CALLS[0]}.json").write_text(json.dumps(stamp))
            pat_root = root / "pat-mail"
            pat_mail = pat_root / CALLS[0]
            (pat_mail / "out").mkdir(parents=True)
            pat_command = [
                args.pat, "--config", config, "--mbox", pat_root,
                "--forms", root / "forms", "--prehooks", root / "prehooks",
                "--log", root / "pat.log", "--event-log", root / "events.json",
                "connect", f"varahf:///{CALLS[1]}?p2p=true",
            ]
        audio = [stack.enter_context(socket.create_connection(("127.0.0.1", p[2])))
                 for p in ports]
        for source, destination in (audio, audio[::-1]):
            threading.Thread(target=relay, args=(source, destination), daemon=True).start()

        if args.mail_bridge:
            reader = mail_client(stack, args.cli, "imap", root / CALLS[1], imaplib.IMAP4)
            assert reader.select("INBOX", readonly=True) == ("OK", [b"0"])
            uid_validity = reader.response("UIDVALIDITY")[1]
            sender_client = mail_client(
                stack, args.cli, "smtp", root / CALLS[0], smtplib.SMTP, CALLS[0]
            )

        messages = []
        for index, (local, peer) in enumerate((CALLS, CALLS[::-1])):
            mid = f"BENCH{index}"
            attachment = hashlib.shake_256(mid.encode()).digest(1400)
            wire = (
                f"Mid: {mid}\r\nDate: 2026/11/13 12:00\r\nType: Private\r\n"
                f"From: {local}\r\nTo: {peer}\r\nSubject: Linkwise bench\r\n"
                f"Body: 5\r\nFile: {len(attachment)} bench.bin\r\n\r\nBench\r\n"
            ).encode() + attachment + b"\r\n"
            source = root / f"{mid}.b2f"
            source.write_bytes(wire)
            if args.mail_bridge and index == 0:
                mail = EmailMessage()
                mail["From"] = f"{local}@winlink.org"
                mail["To"] = f"{peer}@winlink.org"
                mail["Subject"] = "Linkwise bench"
                mail.set_content("Bench")
                mail.add_attachment(attachment, maintype="application",
                                    subtype="octet-stream", filename="bench.bin")
                assert sender_client.send_message(mail) == {}
                queued, = (root / local / "outbox").glob("*.b2f")
                mid = bytes.fromhex(queued.stem).decode("ascii")
                wire = queued.read_bytes()
            elif args.pat and index == 0:
                (pat_mail / "out" / f"{mid}.b2f").write_bytes(wire)
            else:
                subprocess.run([args.cli, "mail", "queue", root / local, source], check=True)
            messages.append((mid, peer, wire, attachment))

        processes = []
        for index in (1, 0):
            command_line = (pat_command if args.pat and index == 0 else
                [args.cli, "mail", "listen" if index else "exchange",
                 root / CALLS[index], CALLS[index], CALLS[1 - index],
                 *map(str, ports[index][:2])])
            process = subprocess.Popen(command_line,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                env=pat_env if args.pat and index == 0 else None,
            )
            stack.callback(stop, process)
            processes.append(process)
            if index:
                time.sleep(0.3)
        for process in reversed(processes):
            out, err = process.communicate(timeout=180)
            assert process.returncode == 0, (process.args, out, err)
            if args.pat and process.args is pat_command:
                assert b"Disconnected." in err, (out, err)
        for mid, peer, wire, original in messages:
            name = mid.encode().hex() + ".b2f"
            if args.pat and peer == CALLS[0]:
                candidates = list((pat_mail / "in").glob("*.b2f"))
                assert len(candidates) == 1, candidates
                received = candidates[0].read_bytes()
            else:
                received = (root / peer / "inbox" / name).read_bytes()
            assert received.split(b"\r\n", 1)[0].lower() == f"Mid: {mid}".encode().lower()
            if not args.pat:
                assert received == wire
            attachment = received[-len(original) - 2:-2]
            checksum = hashlib.sha256(attachment).hexdigest()
            assert checksum == hashlib.sha256(original).hexdigest()
            sender = CALLS[1 - CALLS.index(peer)]
            if args.pat and sender == CALLS[0]:
                assert not list((pat_mail / "out").glob("*.b2f"))
                assert len(list((pat_mail / "sent").glob("*.b2f"))) == 1
            else:
                assert (root / sender / "finished" / name).read_text() == "confirmed\n"
                assert not (root / sender / "outbox" / name).exists()
            print(f"{mid}: SHA-256 {checksum}")
        if args.mail_bridge:
            assert reader.noop()[0] == "OK"
            assert reader.response("EXISTS")[1][-1] == b"1"
            assert reader.uid("search", None, "ALL") == ("OK", [b"1"])
            status, parts = reader.uid("fetch", "1", "(BODY.PEEK[])")
            assert status == "OK"
            mail = BytesParser(policy=policy.default).parsebytes(parts[0][1])
            assert mail["Subject"] == "Linkwise bench"
            attachment, = mail.iter_attachments()
            assert attachment.get_filename() == "bench.bin"
            assert attachment.get_payload(decode=True) == messages[0][3]
            assert reader.select("INBOX", readonly=True) == ("OK", [b"1"])
            assert reader.response("UIDVALIDITY")[1] == uid_validity
            print("SMTP submission arrived in the open IMAP inbox with its exact attachment.")
        print("Both modem ends disconnected cleanly.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cli")
    parser.add_argument("modem")
    parser.add_argument("profile")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--pat", help="use Pat instead of the Linkwise caller")
    mode.add_argument("--mail-bridge", action="store_true",
                      help="submit via SMTP and read the attachment from a live IMAP inbox")
    test(parser.parse_args())
