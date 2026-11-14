#!/usr/bin/env python3
"""Exercise B2F mail and clean disconnect over two virtual BW500 modems."""

import argparse
import contextlib
import hashlib
import imaplib
import base64
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
CALLS = ("KN6MQS", "0000")


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


def test_imap_only(cli):
    with contextlib.ExitStack() as stack:
        mailbox = Path(stack.enter_context(tempfile.TemporaryDirectory()))
        with contextlib.ExitStack() as first_listener:
            reader = mail_client(first_listener, cli, "imap", mailbox, imaplib.IMAP4)
            assert reader.select("INBOX", readonly=True) == ("OK", [b"0"])
            validity = reader.response("UIDVALIDITY")[1]
            for uid, mid in enumerate(("ZZTEST", "AATEST"), 1):
                wire = (
                    f"Mid: {mid}\r\nDate: 2026/11/14 12:00\r\nType: Private\r\n"
                    f"From: KN6MQS\r\nTo: KN6MQS\r\nSubject: {mid}\r\n"
                    "Body: 4\r\nFile: 3 uid.bin\r\n\r\nTest\r\nABC\r\n"
                ).encode()
                (mailbox / "inbox" / f"{mid.encode().hex()}.b2f").write_bytes(wire)
                assert reader.uid("fetch", str(uid), "(BODY.PEEK[HEADER])")[1][0][1].find(
                    f"Subject: {mid}\r\n".encode()
                ) >= 0
            original = reader.uid("fetch", "1", "(BODY.PEEK[])")[1][0][1]
            assert reader.check()[0] == "OK"
            assert reader.unselect()[0] == "OK"
            assert reader.select("INBOX") == ("OK", [b"2"])
            assert reader.uid("store", "1", "+FLAGS", r"(\Seen)") == (
                "OK", [br"1 (UID 1 FLAGS (\Seen))"]
            )
            assert reader.uid("search", None, "SEEN") == ("OK", [b"1"])
            assert reader.uid("search", None, "UNSEEN") == ("OK", [b"2"])
            assert reader.uid("fetch", "2", "(BODY.PEEK[])")[0] == "OK"
            assert reader.uid("search", None, "UNSEEN") == ("OK", [b"2"])
            assert reader.uid("fetch", "2", "(BODY[TEXT])")[0] == "OK"
            assert reader.uid("search", None, "UNSEEN") == ("OK", [b""])
            assert reader.append("Sent", r"(\Seen)", None, b"Subject: Sent\r\n\r\nTest\r\n")[0] == "OK"
        with contextlib.ExitStack() as restarted:
            reopened = mail_client(restarted, cli, "imap", mailbox, imaplib.IMAP4)
            assert reopened.select("INBOX", readonly=True) == ("OK", [b"2"])
            assert reopened.response("UIDVALIDITY")[1] == validity
            assert reopened.response("UIDNEXT")[1] == [b"3"]
            assert reopened.uid("fetch", "1", "(BODY.PEEK[])")[1][0][1] == original
            assert reopened.uid("fetch", "1", "(FLAGS)") == (
                "OK", [br"1 (UID 1 FLAGS (\Seen))"]
            )
            assert reopened.status("INBOX", "(UNSEEN)") == ("OK", [b"INBOX (UNSEEN 0)"])
            assert reopened.status("Sent", "(UNSEEN)") == ("OK", [b"Sent (UNSEEN 0)"])
            assert reopened.uid("store", "1", "-FLAGS", r"(\Seen)")[0] == "NO"
            assert reopened.select("INBOX") == ("OK", [b"2"])
            assert reopened.store("1", "-FLAGS.SILENT", r"(\Seen)") == ("OK", [None])
            assert reopened.status("INBOX", "(UNSEEN)") == ("OK", [b"INBOX (UNSEEN 1)"])
            assert reopened.select("INBOX", readonly=True)[0] == "OK"
            assert reopened.uid("fetch", "1", "(RFC822)")[0] == "OK"
            assert reopened.uid("search", None, "UNSEEN") == ("OK", [b"1"])
            for uid, mid in enumerate(("ZZTEST", "AATEST"), 1):
                status, parts = reopened.uid("fetch", str(uid), "(BODY.PEEK[HEADER])")
                assert status == "OK"
                assert f"Subject: {mid}\r\n".encode() in parts[0][1]
            with socket.create_connection((reopened.host, reopened.port), timeout=5) as client:
                with client.makefile("rb") as replies:
                    assert replies.readline().startswith(b"* PREAUTH")
                    client.sendall(b'x SELECT "unfinished\r\ny NOOP\r\n')
                    assert replies.readline().startswith(b"x BAD")
                    assert replies.readline().startswith(b"y OK")
                    client.sendall(b"z " + b"x" * 4096 + b"\r\n")
                    assert replies.readline().startswith(b"* BYE")
                    assert replies.readline() == b""
        print("IMAP parser and session checks passed without a modem connection.")


def test(args):
    with contextlib.ExitStack() as stack:
        root = Path(stack.enter_context(tempfile.TemporaryDirectory()))
        mailboxes = (root / "caller", root / "listener")
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
            reader = mail_client(stack, args.cli, "imap", mailboxes[1], imaplib.IMAP4)
            assert reader.status('"Inbox"', "(MESSAGES RECENT UIDNEXT UNSEEN)") == (
                "OK", [b"INBOX (MESSAGES 0 RECENT 0 UIDNEXT 1 UNSEEN 0)"]
            )
            assert reader.select("INBOX", readonly=True) == ("OK", [b"0"])
            uid_validity = reader.response("UIDVALIDITY")[1]
            assert reader.status('"INBOX"', "(messages recent uidnext uidvalidity unseen)") == (
                "OK", [b"INBOX (MESSAGES 0 RECENT 0 UIDNEXT 1 UIDVALIDITY "
                       + uid_validity[0] + b" UNSEEN 0)"]
            )
            assert reader.status("Sent", "(MESSAGES UIDNEXT UIDVALIDITY)") == (
                "OK", [b"Sent (MESSAGES 0 UIDNEXT 1 UIDVALIDITY 1)"]
            )
            assert any(line.endswith(b" Sent") for line in reader.list()[1])
            with socket.create_connection((reader.host, reader.port), timeout=5) as client:
                with client.makefile("rb") as replies:
                    assert replies.readline().startswith(b"* PREAUTH")
                    client.sendall(b'x SELECT "unfinished\r\ny NOOP\r\n')
                    assert replies.readline().startswith(b"x BAD")
                    assert replies.readline().startswith(b"y OK")
                    client.sendall(b"a APPEND Sent {2097153}\r\n")
                    assert replies.readline().startswith(b"a NO")
                    client.sendall(b"b APPEND Sent {100}\r\n")
                    assert replies.readline().startswith(b"+")
                    client.sendall(b"interrupted")
            sender_client = mail_client(
                stack, args.cli, "smtp", mailboxes[0], smtplib.SMTP, CALLS[0]
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
                assert reader.append("Sent", r"(\Seen)", None, mail.as_bytes())[0] == "OK"
                queued, = (mailboxes[index] / "outbox").glob("*.b2f")
                mid = bytes.fromhex(queued.stem).decode("ascii")
                wire = queued.read_bytes()
            elif args.pat and index == 0:
                (pat_mail / "out" / f"{mid}.b2f").write_bytes(wire)
            else:
                subprocess.run([args.cli, "mail", "queue", mailboxes[index], source], check=True)
            messages.append((mid, wire, attachment))

        processes = []
        for index in (1, 0):
            command_line = (pat_command if args.pat and index == 0 else
                [args.cli, "mail", "listen" if index else "exchange",
                 mailboxes[index], CALLS[index], CALLS[1 - index],
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
        for index, (mid, wire, original) in enumerate(messages):
            name = mid.encode().hex() + ".b2f"
            if args.pat and index == 1:
                candidates = list((pat_mail / "in").glob("*.b2f"))
                assert len(candidates) == 1, candidates
                received = candidates[0].read_bytes()
            else:
                received = (mailboxes[1 - index] / "inbox" / name).read_bytes()
            assert received.split(b"\r\n", 1)[0].lower() == f"Mid: {mid}".encode().lower()
            if not args.pat:
                assert received == wire
            attachment = received[-len(original) - 2:-2]
            checksum = hashlib.sha256(attachment).hexdigest()
            assert checksum == hashlib.sha256(original).hexdigest()
            if args.pat and index == 0:
                assert not list((pat_mail / "out").glob("*.b2f"))
                assert len(list((pat_mail / "sent").glob("*.b2f"))) == 1
            else:
                assert (mailboxes[index] / "finished" / name).read_text() == "confirmed\n"
                assert not (mailboxes[index] / "outbox" / name).exists()
            print(f"{mid}: SHA-256 {checksum}")
        if args.mail_bridge:
            assert reader.noop()[0] == "OK"
            assert reader.response("EXISTS")[1][-1] == b"1"
            assert reader.status("INBOX", "(MESSAGES UIDNEXT UIDVALIDITY UNSEEN)") == (
                "OK", [b"INBOX (MESSAGES 1 UIDNEXT 2 UIDVALIDITY "
                       + uid_validity[0] + b" UNSEEN 1)"]
            )
            assert reader.uid("search", None, "1:1 NOT DELETED") == ("OK", [b"1"])
            assert reader.uid("fetch", "1", "(UID FLAGS)") == ("OK", [b"1 (UID 1 FLAGS ())"])
            status, parts = reader.fetch("1", "(INTERNALDATE UID RFC822.SIZE FLAGS BODY.PEEK[HEADER])")
            assert status == "OK"
            headers = parts[0][1]
            assert headers.endswith(b"\r\n\r\n")
            assert b"\r\n\r\n" not in headers[:-4]
            assert BytesParser(policy=policy.default).parsebytes(headers)["Subject"] == "Linkwise bench"
            status, parts = reader.uid("fetch", "1", "(UID BODYSTRUCTURE)")
            assert status == "OK"
            assert b'"text" "plain"' in parts[0]
            assert b'"application" "octet-stream"' in parts[0]
            assert b'"filename" "bench.bin"' in parts[0]
            status, parts = reader.uid("fetch", "1", "(UID BODY.PEEK[1] BODY.PEEK[2])")
            assert status == "OK"
            text, binary = [literal for record in parts if isinstance(record, tuple)
                            for literal in [record[1]]]
            assert text.strip() == b"Bench"
            assert base64.b64decode(binary) == messages[0][2]
            status, parts = reader.uid("fetch", "1", "(BODY.PEEK[])")
            assert status == "OK"
            mail = BytesParser(policy=policy.default).parsebytes(parts[0][1])
            assert mail["Subject"] == "Linkwise bench"
            attachment, = mail.iter_attachments()
            assert attachment.get_filename() == "bench.bin"
            assert attachment.get_payload(decode=True) == messages[0][2]
            assert reader.select("INBOX", readonly=True) == ("OK", [b"1"])
            assert reader.response("UIDVALIDITY")[1] == uid_validity
            assert reader.select("Sent", readonly=True) == ("OK", [b"1"])
            assert reader.response("UIDVALIDITY")[1] == [b"1"]
            status, parts = reader.uid("fetch", "1", "(BODY.PEEK[])")
            assert status == "OK"
            sent = BytesParser(policy=policy.default).parsebytes(parts[0][1])
            saved, = sent.iter_attachments()
            assert saved.get_payload(decode=True) == messages[0][2]
            reopened = mail_client(stack, args.cli, "imap", mailboxes[1], imaplib.IMAP4)
            assert reopened.select("Sent", readonly=True) == ("OK", [b"1"])
            assert reopened.response("UIDVALIDITY")[1] == [b"1"]
            assert reopened.uid("fetch", "1", "(BODY.PEEK[])") == (status, parts)
            assert reopened.check()[0] == "OK"
            assert reopened.unselect()[0] == "OK"
            print("SMTP submission arrived in the open IMAP inbox with its exact attachment.")
        print("Both modem ends disconnected cleanly.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cli")
    parser.add_argument("modem", nargs="?")
    parser.add_argument("profile", nargs="?")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--pat", help="use Pat instead of the Linkwise caller")
    mode.add_argument("--mail-bridge", action="store_true",
                      help="submit via SMTP and read the attachment from a live IMAP inbox")
    mode.add_argument("--imap-only", action="store_true",
                      help="verify IMAP without opening a modem connection")
    args = parser.parse_args()
    if args.imap_only:
        test_imap_only(args.cli)
    elif not args.modem or not args.profile:
        parser.error("modem and profile are required for virtual BW500 testing")
    else:
        test(args)
