#!/usr/bin/env python3
"""Exercise shared PKI with Winlink mail and interactive BW500 chat."""

import argparse
import base64
import hashlib
import json
import os
import runpy
import socket
import subprocess
import tempfile
import threading
from contextlib import ExitStack
from pathlib import Path

CHAT = runpy.run_path(str(Path(__file__).with_name("test-varac-chat.py")))
CALLS = ("KN6MQS", "BB2BB")


def run(*args, cwd=None, check=True, input=None):
    result = subprocess.run(args, cwd=cwd, capture_output=True, input=input)
    if check and result.returncode:
        raise AssertionError((args, result.returncode, result.stdout, result.stderr))
    return result


def certificate_fingerprint(path):
    der = run("openssl", "x509", "-in", path, "-outform", "DER").stdout
    return hashlib.sha256(der).hexdigest()


def ca_fingerprint(path):
    public_key = run("openssl", "x509", "-in", path, "-pubkey", "-noout").stdout
    return hashlib.sha256(base64.b64decode(b"".join(public_key.splitlines()[1:-1]))).hexdigest()


def issue(directory, ca_name="Linkwise bench CA"):
    (directory / "certs").mkdir()
    (directory / "index").touch()
    (directory / "serial").write_text("10\n")
    (directory / "crlnumber").write_text("10\n")
    conf = directory / "ca.conf"
    conf.write_text("""[ca]
default_ca = issuer
[req]
distinguished_name = names
[issuer]
database = index
new_certs_dir = certs
serial = serial
crlnumber = crlnumber
certificate = ca.pem
private_key = ca.key
default_md = sha256
default_days = 2
default_crl_days = 1
policy = names
[names]
commonName = supplied
[root]
basicConstraints = critical,CA:TRUE
keyUsage = critical,keyCertSign,cRLSign
subjectKeyIdentifier = hash
""")
    run("openssl", "genpkey", "-algorithm", "EC", "-pkeyopt", "ec_paramgen_curve:prime256v1",
        "-pkeyopt", "ec_param_enc:named_curve",
        "-out", "ca.key", cwd=directory)
    run("openssl", "req", "-new", "-x509", "-sha256", "-days", "2", "-key", "ca.key",
        "-subj", f"/CN={ca_name}", "-out", "ca.pem", "-config", conf,
        "-extensions", "root", cwd=directory)
    for name in CALLS:
        for role, usage in (("sign", "digitalSignature"), ("encrypt", "keyAgreement")):
            stem = f"{name}-{role}"
            run("openssl", "genpkey", "-algorithm", "EC", "-pkeyopt",
                "ec_paramgen_curve:prime256v1", "-pkeyopt", "ec_param_enc:named_curve",
                "-out", f"{stem}.key", cwd=directory)
            run("openssl", "req", "-new", "-config", conf, "-key", f"{stem}.key",
                "-subj", f"/CN={stem}", "-out", f"{stem}.csr", cwd=directory)
            ext = directory / "leaf.conf"
            ext.write_text("[leaf]\nbasicConstraints=critical,CA:FALSE\n"
                           f"keyUsage=critical,{usage}\n"
                           f"subjectAltName=DNS:{name.lower()}.radio.linkwise.invalid\n"
                           "authorityKeyIdentifier=keyid\n")
            run("openssl", "ca", "-batch", "-notext", "-config", conf, "-extfile", ext,
                "-extensions", "leaf", "-in", f"{stem}.csr", "-out", f"{stem}.pem", cwd=directory)
    run("openssl", "ca", "-gencrl", "-config", conf, "-out", "ca.crl", cwd=directory)


def profile(directory, name, peer, openssl=None, provider_config=None):
    document = dict(
        version=1, callsign=name,
        signing=dict(certificate=f"{name}-sign.pem", private_key=f"{name}-sign.key"),
        authorities=["ca.pem"], revocation_lists=["ca.crl"],
        peers={peer: dict(signing_certificate=f"{peer}-sign.pem")},
    )
    if openssl:
        document["peers"][peer]["encryption_certificate"] = f"{peer}-encrypt.pem"
        document["encryption"] = dict(
            openssl=openssl, openssl_config=provider_config, allow_mail=True, allow_chat=True,
            credential=dict(certificate=f"{name}-encrypt.pem", private_key=f"{name}-encrypt.key"),
        )
    path = directory / f"{name}.json"
    path.write_text(json.dumps(document))
    return path


def mail(cli, directory, profiles, encrypted, submission=False):
    sender, recipient = CALLS
    original = directory / "message.eml"
    original.write_bytes(
        f"From: {sender}@winlink.org\r\nTo: {recipient}@winlink.org\r\n"
        "Subject: PKI test\r\nMIME-Version: 1.0\r\n"
        "Content-Type: multipart/mixed; boundary=sample\r\n\r\n"
        "--sample\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n"
        "Signed content from the bench.\r\n"
        "--sample\r\nContent-Type: application/octet-stream\r\n"
        "Content-Disposition: attachment; filename=sample.bin\r\n"
        "Content-Transfer-Encoding: base64\r\n\r\nAAEC/w==\r\n"
        "--sample--\r\n".encode()
    )
    mailbox = directory / ("submission-mailbox" if submission else "mailbox")
    flags = ["--encrypt"] if encrypted else []
    if submission:
        command = [cli, "mail", "submit", "--pki", profiles[0], mailbox]
        draft = original.read_bytes()
        unknown = b"CC3CC@winlink.org"
        mismatch = run(*command, unknown, input=draft, check=False)
        assert mismatch.returncode and b"differs from the message To header" in mismatch.stderr, mismatch
        rejected = run(*command, unknown, input=draft.replace(f"{recipient}@winlink.org".encode(), unknown),
                       check=False)
        assert rejected.returncode and not list((mailbox / "outbox").iterdir()), rejected
        run(*command, f"{recipient}@winlink.org", input=draft)
    else:
        run(cli, "mail", "import", "--pki", profiles[0], *flags, mailbox, original)
    queued = next((mailbox / "outbox").glob("*.b2f"))
    envelope = queued.read_bytes()
    assert b"Signed content from the bench" not in envelope
    received = mailbox / "inbox" / queued.name
    received.write_bytes(envelope.replace(b"Linkwise protected radio mail", b"Subject rewritten by relay  "))
    mid = bytes.fromhex(queued.stem).decode("ascii")
    result = run(cli, "mail", "export", "--pki", profiles[1], *flags, mailbox, mid)
    assert b"Verified radio email sender" in result.stderr
    assert b"Signed content from the bench" in result.stdout and b"AAEC/w==" in result.stdout
    if not encrypted:
        failure = run(cli, "mail", "export", "--pki", profiles[1], "--encrypt",
                      mailbox, mid, check=False)
        assert failure.returncode and not failure.stdout
    marker = b'"content":"' if encrypted else b'"signature":"'
    at = envelope.index(marker) + len(marker)
    modified = b"A" if envelope[at] != ord("A") else b"B"
    received.write_bytes(envelope[:at] + modified + envelope[at+1:])
    failure = run(cli, "mail", "export", "--pki", profiles[1], *flags, mailbox,
                  mid, check=False)
    assert failure.returncode and not failure.stdout
    mode = "encrypted submission from stdin" if submission else "encrypted" if encrypted else "signed"
    print(f"mail ({mode}): verified message and attachment, rejected tampering")


def chat_stations(stack, args, options):
    ports = [CHAT["bench"].start_station(stack, args.modem, args.profile, call, 500)
             for call in CALLS]
    audio = [stack.enter_context(socket.create_connection(("127.0.0.1", port[2])))
             for port in ports]
    for source, destination in (audio, audio[::-1]):
        threading.Thread(target=CHAT["relay"], args=(source, destination), daemon=True).start()
    if getattr(args, "data_adapter", None):
        ports = [args.data_adapter(stack, side, port) for side, port in zip(("left", "right"), ports)]
    return [CHAT["Chat"](stack, args.cli, call, port, flags)
            for call, port, flags in zip(CALLS, ports, options)]


def chat(args, configs, encrypted):
    with ExitStack() as stack:
        flags = ["--encrypt"] if encrypted else []
        a, b = chat_stations(stack, args, [["--pki", str(config), *flags] for config in configs])
        b.enter("/listen on")
        b.expect("Listening enabled")
        a.enter(f"/connect {CALLS[1]}")
        a.expect(f"Verified peer: {CALLS[1]}", seconds=90)
        b.expect(f"Verified peer: {CALLS[0]}", seconds=90)
        a.expect(f"{CALLS[1]} joined the chat (verified)")
        b.expect(f"{CALLS[0]} joined the chat (verified)")
        if encrypted:
            for terminal in (a, b):
                terminal.expect("Encryption enabled.", seconds=90)
            a.expect(f"connected {CALLS[1]} | listen off | verified, encrypted")
        indicator = "|E" if encrypted else "|V"
        a.enter("First authenticated line")
        a.expect(f"<{CALLS[0]:9} {indicator}> First authenticated line")
        b.expect(f"<{CALLS[0]:9} {indicator}> First authenticated line", seconds=90)
        proof = configs[1].parent / ("encrypted-chat-proof.json" if encrypted else "signed-chat-proof.json")
        b.enter(f"/proof {proof}")
        b.expect("Saved signed message")
        verified = run(args.cli, "pki", "verify-chat", configs[1], proof)
        record = json.loads(verified.stdout)
        assert record["verified_sender"] == CALLS[0]
        assert record["message"]["text"] == "First authenticated line"
        stored = json.loads(proof.read_bytes())
        stored["signature"] = "AA=="
        proof.write_text(json.dumps(stored))
        rejected = run(args.cli, "pki", "verify-chat", configs[1], proof, check=False)
        assert rejected.returncode != 0 and not rejected.stdout
        b.enter("Authenticated reply")
        a.expect(f"<{CALLS[1]:9} {indicator}> Authenticated reply", seconds=90)
        a.enter("/disconnect")
        a.expect(f"{CALLS[1]} left the chat")
        b.expect(f"{CALLS[0]} disconnected")
        for terminal in (a, b):
            terminal.quit()
    print(f"chat ({'encrypted' if encrypted else 'signed'}): mutual authentication, exact text, and tamper-checked proof export")


def rejected_chat(args, configs, encrypted):
    caller = ["--pki", str(configs[0]), "--encrypt"] if encrypted else []
    with ExitStack() as stack:
        a, b = chat_stations(stack, args, [caller, ["--pki", str(configs[1])]])
        b.enter("/listen on")
        b.expect("Listening enabled")
        a.enter(f"/connect {CALLS[1]}")
        if encrypted:
            b.expect(f"<{CALLS[0]:9} |F> Encryption failed", seconds=90)
        else:
            a.expect("Connected at BW500")
            a.enter("Unsigned text")
            b.expect(f"<{CALLS[0]:9} |X> Verification failed", seconds=90)
        b.expect("Disconnected")
        b.quit()
    print("chat: rejected mismatched encryption mode" if encrypted else "chat: rejected unsigned text")


def chat_encryption_upgrade(args, configs, provisioned):
    with ExitStack() as stack:
        options = [["--pki", str(config)] for config in configs]
        a, b = chat_stations(stack, args, options)
        b.enter("/listen on")
        b.expect("Listening enabled")
        a.enter(f"/connect {CALLS[1]}")
        a.expect(f"Verified peer: {CALLS[1]}", seconds=90)
        b.expect(f"Verified peer: {CALLS[0]}", seconds=90)
        a.enter("Signed before the upgrade")
        a.expect(f"<{CALLS[0]:9} |V> Signed before the upgrade")
        b.expect(f"<{CALLS[0]:9} |V> Signed before the upgrade", seconds=90)
        a.expect(f"connected {CALLS[1]} | listen off | verified, signed")
        a.enter("/encrypt")
        if provisioned:
            b.expect("Peer requested encryption.", seconds=90)
            a.enter("Held until the upgrade")
            a.expect("Text kept as draft.")
            b.enter("/encrypt")
            profile = json.loads(configs[0].read_text())
            algorithm = ("P-256 + ML-KEM-768" if profile.get("chat_key_exchange") == "p256_mlkem768"
                         else "P-256")
            for terminal in (a, b):
                terminal.expect(f"Encryption enabled. TLS 1.3 with {algorithm}.", seconds=90)
            a.expect(f"connected {CALLS[1]} | listen off | verified, encrypted")
            a.enter("")
            b.expect(f"<{CALLS[0]:9} |E> Held until the upgrade", seconds=90)
            b.expect(f"connected {CALLS[0]} | listen on | verified, encrypted")
            b.enter("Encrypted reply")
            a.expect(f"<{CALLS[1]:9} |E> Encrypted reply", seconds=90)
            a.enter("/disconnect")
        else:
            a.expect(f"<{CALLS[1]:9} |F> Encryption failed")
        for terminal in (a, b):
            terminal.expect(" | idle | ")
            terminal.quit()
    print("chat: upgraded in place after peer consent, preserved draft" if provisioned
          else "chat: upgrade without encryption credentials failed closed")


def reject_classical_downgrade(args, configs):
    classical = configs[1].with_name("classical-only.json")
    document = json.loads(configs[1].read_text())
    document["chat_key_exchange"] = "p256"
    classical.write_text(json.dumps(document))
    with ExitStack() as stack:
        a, b = chat_stations(stack, args, [
            ["--pki", str(configs[0]), "--encrypt"],
            ["--pki", str(classical), "--encrypt"],
        ])
        b.enter("/listen on")
        b.expect("Listening enabled")
        a.enter(f"/connect {CALLS[1]}")
        b.expect(f"<{CALLS[0]:9} |F> Encryption failed", seconds=90)
        b.expect(" | idle | ")
        b.quit()
    print("chat: hybrid TLS rejected a classical-only peer")


def enrollment(args, root):
    configs = []
    encrypted = bool(args.openssl_fips)
    for call, peer in (CALLS, CALLS[::-1]):
        directory = root / call
        directory.mkdir()
        issue(directory, f"{call} bench CA")
        config = profile(directory, call, peer, args.openssl_fips, args.openssl_config)
        document = json.loads(config.read_text())
        document["authorities"].append(f"../{peer}/ca.pem")
        document["revocation_lists"].append(f"../{peer}/ca.crl")
        # The caller starts with an older peer certificate. The recipient has a new peer.
        if call == CALLS[1]:
            document["peers"] = {}
        if not encrypted:
            document["allow_chat_encryption"] = True
            if call == CALLS[0]:
                document["peers"][peer]["require_encryption"] = True
        config.write_text(json.dumps(document))
        configs.append(config)
    for config, peer in zip(configs, CALLS[::-1]):
        document = json.loads(config.read_text())
        ca_path = root / peer / "ca.pem"
        fingerprint = run(args.cli, "pki", "fingerprint", "ca", ca_path).stdout.decode().strip()
        assert fingerprint.replace(":", "").lower() == ca_fingerprint(ca_path)
        document["peer_authorities"] = {peer: fingerprint}
        config.write_text(json.dumps(document))

    with ExitStack() as stack:
        a, b = chat_stations(stack, args, [["--pki", str(config), "--enroll"] for config in configs])
        b.enter("/listen on")
        b.expect("Listening enabled")
        a.enter(f"/connect {CALLS[1]}")
        for terminal in (a, b):
            terminal.expect("Enrollment ready.", seconds=90)
        b.enter("/cert accept")
        b.expect("use /cert inspect and compare fingerprints")
        offer = "/cert offer --encrypt" if encrypted else "/cert offer"
        for sender, receiver, sender_call in ((a, b, CALLS[0]), (b, a, CALLS[1])):
            sender.enter(offer)
            receiver.expect(f"Certificate offer from {sender_call}.", seconds=90)
            receiver.enter("/cert inspect")
            receiver.expect("Offered signing certificate SHA-256")
            if encrypted:
                receiver.expect("Offered encryption certificate SHA-256")
            fingerprints = [certificate_fingerprint(root / sender_call / f"{sender_call}-sign.pem")]
            if encrypted:
                fingerprints.append(certificate_fingerprint(root / sender_call / f"{sender_call}-encrypt.pem"))
            confirmed = " ".join(fingerprints)
            if receiver is a:
                receiver.expect("use /cert accept replace SIGNING")
                receiver.enter(f"/cert accept {confirmed}")
                receiver.expect("this offer changes an installed certificate")
                receiver.enter(f"/cert accept replace {confirmed}")
            else:
                wrong = "0" * 64
                receiver.enter("/cert accept " + " ".join([wrong, *fingerprints[1:]]))
                receiver.expect("out-of-band fingerprint does not match")
                receiver.enter(f"/cert accept {confirmed}")
            receiver.expect("Certificates saved to the shared PKI profile.")
        a.enter("/disconnect")
        for terminal in (a, b):
            terminal.expect(" | idle | ")
            terminal.quit()

    for config, peer in zip(configs, CALLS[::-1]):
        stored = json.loads(config.read_text())["peers"][peer]
        assert "der" in stored["signing_certificate"]
        if encrypted:
            assert "der" in stored["encryption_certificate"]
        run(args.cli, "pki", "check", config)
    if not encrypted:
        document = json.loads(configs[0].read_text())
        renewed = document["peers"][CALLS[1]]
        assert renewed["require_encryption"] and renewed["encryption_certificate"] is None
        chat(args, configs, encrypted=True)
        # The signed mail bench uses a profile that permits unencrypted mail.
        renewed["require_encryption"] = False
        configs[0].write_text(json.dumps(document))
    mail(args.cli, root, configs, encrypted)
    chat(args, configs, encrypted)
    print("enrollment: two CAs, explicit first acceptance and rotation, verified reuse in chat and mail")
    rejected_enrollment(args, configs)


def rejected_enrollment(args, configs):
    restricted = configs[0].with_name("untrusted.json")
    document = json.loads(configs[0].read_text())
    document["peer_authorities"][CALLS[1]] = ca_fingerprint(configs[0].with_name("ca.pem"))
    restricted.write_text(json.dumps(document))
    original = restricted.read_bytes()
    with ExitStack() as stack:
        a, b = chat_stations(stack, args, [
            ["--pki", str(restricted), "--enroll"],
            ["--pki", str(configs[1]), "--enroll"],
        ])
        b.enter("/listen on")
        b.expect("Listening enabled")
        a.enter(f"/connect {CALLS[1]}")
        for terminal in (a, b):
            terminal.expect("Enrollment ready.", seconds=90)
        b.enter("/cert offer")
        a.expect(f"<{CALLS[1]:9} |X> Verification failed", seconds=90)
        a.expect("UnknownIssuer")
        a.expect(" | idle | ")
        a.quit()
    assert restricted.read_bytes() == original
    print("enrollment: rejected an offer from a trusted CA not authorized for this callsign")


def revocation_refresh(args, directory, configs):
    watched = directory / "watched.crl"
    watched.write_bytes((directory / "ca.crl").read_bytes())
    document = json.loads(configs[0].read_text())
    document["revocation_lists"] = [watched.name]
    configs[0].write_text(json.dumps(document))
    with ExitStack() as stack:
        a, b = chat_stations(stack, args, [["--pki", str(config)] for config in configs])
        b.enter("/listen on")
        b.expect("Listening enabled")
        a.enter(f"/connect {CALLS[1]}")
        a.expect(f"{CALLS[1]} joined the chat (verified)", seconds=90)
        b.expect(f"{CALLS[0]} joined the chat (verified)", seconds=90)
        run("openssl", "ca", "-config", "ca.conf", "-revoke", f"{CALLS[1]}-sign.pem", cwd=directory)
        run("openssl", "ca", "-gencrl", "-config", "ca.conf", "-out", "fresh.crl", cwd=directory)
        (directory / "fresh.crl").replace(watched)
        a.expect(f"<{CALLS[1]:9} |X> Verification failed", seconds=45)
        a.expect("Revoked")
        a.expect(" | idle | ")
        a.quit()
    print("chat: revoked signing certificate disconnected an active call after refresh")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cli")
    parser.add_argument("modem")
    parser.add_argument("profile")
    parser.add_argument("--fixture-directory", type=Path)
    parser.add_argument("--openssl-fips")
    parser.add_argument("--openssl-config")
    parser.add_argument("--skip-chat", action="store_true")
    parser.add_argument("--enrollment-only", action="store_true")
    parser.add_argument("--pqc-upgrade-only", action="store_true")
    args = parser.parse_args()
    if bool(args.openssl_fips) != bool(args.openssl_config):
        parser.error("supply both OpenSSL FIPS arguments for encrypted tests")
    with ExitStack() as stack:
        directory = args.fixture_directory or Path(stack.enter_context(tempfile.TemporaryDirectory()))
        os.environ["TMPDIR"] = str(directory)
        if args.enrollment_only:
            enrollment(args, directory)
            raise SystemExit
        issue(directory)
        configs = [profile(directory, call, peer, args.openssl_fips, args.openssl_config)
                   for call, peer in (CALLS, CALLS[::-1])]
        if args.pqc_upgrade_only:
            for config in configs:
                document = json.loads(config.read_text())
                document.update(allow_chat_encryption=True, chat_key_exchange="p256_mlkem768")
                config.write_text(json.dumps(document))
                run(args.cli, "pki", "check", config)
            chat_encryption_upgrade(args, configs, True)
            reject_classical_downgrade(args, configs)
            raise SystemExit
        for config in configs:
            run(args.cli, "pki", "check", config)
        encrypted = bool(args.openssl_fips)
        mail(args.cli, directory, configs, encrypted)
        if encrypted:
            mail(args.cli, directory, configs, encrypted=True, submission=True)
        if not args.skip_chat:
            chat(args, configs, encrypted)
            rejected_chat(args, configs, encrypted)
            chat_encryption_upgrade(args, configs, encrypted)
            revocation_refresh(args, directory, configs)
        else:
            run("openssl", "ca", "-config", "ca.conf", "-revoke", f"{CALLS[1]}-sign.pem", cwd=directory)
            run("openssl", "ca", "-gencrl", "-config", "ca.conf", "-out", "ca.crl", cwd=directory)
        result = run(args.cli, "pki", "check", configs[0], check=False)
        assert result.returncode and b"Revoked" in result.stderr, result.stderr
        print("revoked signing certificate rejected")
