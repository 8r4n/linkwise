#!/usr/bin/env python3
"""Render and import a QSL card through the production commands.

Requires a secure-transfer Linkwise build, openssl, and Poppler's pdftoppm
and pdfinfo. Uses separate test CAs and local files only.
"""

import argparse
import json
import runpy
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

PKI = runpy.run_path(str(Path(__file__).with_name("test-pki.py")))
CONTACT = [
    "--utc", "2026-11-15T19:30:00Z", "--freq-khz", "7105",
    "--mode", "BW500", "--rst", "59", "--grid", "CM87",
    "--notes", "Thanks for the QSO.",
]


def exercise(cli, root, sample, openssl, openssl_config):
    sender, recipient = PKI["CALLS"]
    outgoing, incoming, drop = (root / name for name in ("sender", "recipient", "photos"))
    for directory in (outgoing, incoming, drop):
        directory.mkdir()
    PKI["issue"](outgoing, "QSL test sender CA")
    PKI["issue"](incoming, "QSL test recipient CA")
    source = PKI["profile"](outgoing, sender, recipient, openssl, openssl_config)
    target = PKI["profile"](incoming, recipient, sender, openssl, openssl_config)
    for profile, peer_dir, peer in ((source, incoming, recipient), (target, outgoing, sender)):
        directory = profile.parent
        shutil.copyfile(peer_dir / "ca.pem", directory / "peer-ca.pem")
        shutil.copyfile(peer_dir / "ca.crl", directory / "peer-ca.crl")
        config = json.loads(profile.read_text())
        config["authorities"].append("peer-ca.pem")
        config["revocation_lists"].append("peer-ca.crl")
        config["peer_authorities"] = {peer: PKI["ca_fingerprint"](directory / "peer-ca.pem")}
        config["peers"] = {}
        profile.write_text(json.dumps(config))
    pristine = target.read_bytes()
    fingerprints = [PKI["certificate_fingerprint"](outgoing / f"{sender}-{role}.pem")
                    for role in ("sign", "encrypt")]

    def invoke(*args, check=True):
        return PKI["run"](cli, "qsl", *args, check=check)

    def card(name, destination, *options, profile=source):
        pdf = root / f"{name}.pdf"
        invoke("card", profile, destination, pdf, *CONTACT, *options)
        PKI["run"]("pdftoppm", "-singlefile", "-r", "300", "-jpeg", pdf, root / name)
        return pdf, root / f"{name}.jpg"

    pdf, scan = card("qsl", recipient)
    assert b"396 x 252 pts" in PKI["run"]("pdfinfo", pdf).stdout
    assert invoke("card", source, recipient, pdf, *CONTACT, check=False).returncode
    if sample:
        shutil.copyfile(pdf, sample)

    inspection = invoke("inspect", scan).stdout.decode()
    assert f"QSL card from {sender} to {recipient} (unverified)" in inspection
    normalized = inspection.replace(":", "").upper()
    assert all(fingerprint.upper() in normalized for fingerprint in fingerprints)

    rejected = invoke("import", target, sender, scan, "00" * 32, fingerprints[1], check=False)
    assert b"fingerprint does not match" in rejected.stderr and target.read_bytes() == pristine
    _, wrong = card("wrong-recipient", "CC3CC")
    rejected = invoke("import", target, sender, wrong, *fingerprints, check=False)
    assert b"wrong signed recipient" in rejected.stderr and target.read_bytes() == pristine
    unpinned = json.loads(pristine)
    unpinned["peer_authorities"] = {}
    target.write_text(json.dumps(unpinned))
    rejected = invoke("import", target, sender, scan, *fingerprints, check=False)
    assert b"configure the CA key fingerprint" in rejected.stderr
    target.write_bytes(pristine)

    shutil.copyfile(scan, drop / "already-present.jpg")
    receiver = subprocess.Popen(
        (cli, "qsl", "receive", target, sender, drop, *fingerprints),
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
    )
    try:
        assert receiver.stdout.readline().startswith("Waiting for a new JPEG")
        time.sleep(1.6)
        assert receiver.poll() is None and target.read_bytes() == pristine
        shutil.copyfile(scan, drop / "shared-from-phone.jpg")
        stdout, stderr = receiver.communicate(timeout=15)
        assert receiver.returncode == 0, (stdout, stderr)
        assert "Installed verified signing and mail-encryption" in stdout
    finally:
        if receiver.poll() is None:
            receiver.kill()
            receiver.communicate()
    installed = target.read_bytes()
    peer = json.loads(installed)["peers"][sender]
    assert len(peer["signing_certificate"]["der"]) == len(peer["encryption_certificate"]["der"]) == 1
    invoke("import", target, sender, scan, *fingerprints)
    assert target.read_bytes() == installed

    wide, signing = card("signing-only", recipient, "--size", "4x6", "--signing-only")
    assert b"432 x 288 pts" in PKI["run"]("pdfinfo", wide).stdout
    invoke("import", target, sender, signing, fingerprints[0])
    assert target.read_bytes() == installed
    PKI["run"](cli, "pki", "check", target)
    _, reverse = card("reply", sender, profile=target)
    reverse_fingerprints = [PKI["certificate_fingerprint"](incoming / f"{recipient}-{role}.pem")
                            for role in ("sign", "encrypt")]
    invoke("import", source, recipient, reverse, *reverse_fingerprints)
    PKI["mail"](cli, root, [source, target], encrypted=True)
    print("QSL: both paper sizes and QR photo imports verified, including separate CAs and automatic receipt")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("linkwise", type=Path)
    parser.add_argument("--openssl", required=True, type=Path)
    parser.add_argument("--openssl-config", required=True, type=Path)
    parser.add_argument("--sample", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="linkwise-qsl-") as temporary:
        exercise(args.linkwise.resolve(), Path(temporary), args.sample,
                 str(args.openssl.resolve()), str(args.openssl_config.resolve()))


if __name__ == "__main__":
    main()
