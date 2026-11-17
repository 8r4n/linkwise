#!/usr/bin/env python3
"""Demonstrate chat and Open B2F mail using virtual audio and loopback only."""
from contextlib import ExitStack
from email import policy
from email.message import EmailMessage
from email.parser import BytesParser
import hashlib
from pathlib import Path
import runpy
import socket
import subprocess
import tempfile
import threading

HERE = Path(__file__).resolve().parent
CLI = "/usr/bin/linkwise"
MODEM = "/usr/bin/linkwise-modemd"
CALLS = ("AA1AA", "BB2BB")


def mail_exchange(profile):
    bench = runpy.run_path(str(HERE / "test-varac-chat.py"))
    attachment = b"Linkwise live media fixture\n"
    message = EmailMessage(policy=policy.SMTP)
    message["From"], message["To"] = (call + "@winlink.org" for call in CALLS)
    message["Subject"] = "Linkwise live USB test"
    message.set_content("Virtual BW500 audio and loopback sockets.")
    message.add_attachment(attachment, maintype="application", subtype="octet-stream", filename="fixture.bin")

    with ExitStack() as stack:
        root = Path(stack.enter_context(tempfile.TemporaryDirectory()))
        outgoing = root / "test.eml"
        outgoing.write_bytes(message.as_bytes())
        sender, recipient = (root / call for call in CALLS)
        ports = [bench["bench"].start_station(stack, MODEM, profile, call, 500) for call in CALLS]
        audio = [stack.enter_context(socket.create_connection(("127.0.0.1", p[2]))) for p in ports]
        for inbound, outbound in (audio, audio[::-1]):
            threading.Thread(target=bench["relay"], args=(inbound, outbound), daemon=True).start()

        subprocess.run([CLI, "mail", "import", sender, outgoing], check=True)
        listen = [CLI, "mail", "listen", recipient, *CALLS[::-1], *map(str, ports[1][:2])]
        exchange = [CLI, "mail", "exchange", sender, *CALLS, *map(str, ports[0][:2])]
        with subprocess.Popen(listen) as peer:
            try:
                subprocess.run(exchange, check=True, timeout=150)
                if peer.wait(timeout=15):
                    raise RuntimeError("B2F listener did not complete and disconnect")
            finally:
                if peer.poll() is None:
                    peer.kill()

        envelope, = (recipient / "inbox").glob("*.b2f")
        message_id = next(line.partition(b":")[2].strip().decode("ascii")
                          for line in envelope.read_bytes().splitlines() if line.lower().startswith(b"mid:"))
        mime = subprocess.check_output([CLI, "mail", "export", recipient, message_id], timeout=10)
        received = BytesParser(policy=policy.default).parsebytes(mime)
        received_attachment, = received.iter_attachments()
        digest = hashlib.sha256(received_attachment.get_payload(decode=True)).hexdigest()
        if received["Subject"] != message["Subject"] or digest != hashlib.sha256(attachment).hexdigest():
            raise RuntimeError("received mail or attachment does not match its source")
        return digest


def main():
    profile = HERE / "bw500-level4.json"
    if not profile.is_file():
        raise SystemExit("Stage the verified BW500 measurement profile before running the offline demo.")
    subprocess.run([CLI, "--version"], check=True)
    subprocess.run(["python3", HERE / "test-varac-chat.py", CLI, MODEM, profile], check=True, timeout=180)
    print(f"Linkwise live smoke passed: chat, B2F mail, attachment SHA-256 {mail_exchange(profile)}")


if __name__ == "__main__":
    main()
