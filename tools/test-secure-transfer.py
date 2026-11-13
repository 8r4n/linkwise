#!/usr/bin/env python3
"""Exercise the secure CLI over two Linkwise virtual BW500 audio endpoints."""

import argparse
import contextlib
import hashlib
import json
import runpy
import socket
import subprocess
import tempfile
import threading
import time
from pathlib import Path

BENCH = runpy.run_path(str(Path(__file__).with_name("test-virtual-bw500.py")))
CALLS = ("KN6MQS", "W1AW")


def openssl(directory, *args):
    subprocess.run(
        ["openssl", *args], cwd=directory, check=True,
        stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
    )


def certificates(directory):
    for name in ("ca", "foreign"):
        openssl(directory, "ecparam", "-name", "prime256v1", "-genkey", "-out", name + ".key")
        openssl(directory, "req", "-new", "-x509", "-sha256", "-days", "1",
                "-key", name + ".key", "-subj", "/CN=" + name, "-out", name + ".pem")
    for name, ca in [(call, "ca") for call in (*CALLS, "OTHER")] + [("UNTRUSTED", "foreign")]:
        openssl(directory, "ecparam", "-name", "prime256v1", "-genkey", "-out", name + ".key")
        openssl(directory, "req", "-new", "-key", name + ".key",
                "-subj", "/CN=" + name, "-out", name + ".csr")
        (directory / "extensions").write_text(
            f"subjectAltName=DNS:{name.lower()}.test\nextendedKeyUsage=clientAuth,serverAuth\n"
            "basicConstraints=CA:FALSE\nkeyUsage=digitalSignature\n"
        )
        openssl(directory, "x509", "-req", "-in", name + ".csr", "-CA", ca + ".pem",
                "-CAkey", ca + ".key", "-CAcreateserial", "-sha256", "-days", "1",
                "-extfile", "extensions", "-out", name + ".pem")


def relay(source, destination):
    try:
        while data := source.recv(65536):
            destination.sendall(data)
    except OSError:
        pass  # Closing either virtual modem ends the relay.


def stop(process):
    if process.poll() is None:
        process.kill()
        process.wait()


def test_pair(args, directory, scenario):
    with contextlib.ExitStack() as stack:
        ports = [BENCH["start_station"](stack, args.modem, args.profile, call, 500) for call in CALLS]
        audio = [
            stack.enter_context(socket.create_connection(("127.0.0.1", port[2]), timeout=5))
            for port in ports
        ]
        for source, destination in (audio, audio[::-1]):
            source.settimeout(None)
            threading.Thread(target=relay, args=(source, destination), daemon=True).start()

        configs = []
        for index, (call, peer) in enumerate((CALLS, CALLS[::-1])):
            settings = dict(
                local_call=call, peer_call=peer, peer_dns=peer.lower() + ".test",
                certificate=str(directory / f"{call}.pem"),
                private_key=str(directory / f"{call}.key"),
                peer_certificate=str(directory / f"{peer}.pem"),
                ca_certificate=str(directory / "ca.pem"),
                command_port=ports[index][0], data_port=ports[index][1], max_bytes=4096,
            )
            if index == 0 and scenario == "pin":
                settings["peer_certificate"] = str(directory / "OTHER.pem")
            if index == 0 and scenario == "dns":
                settings["peer_dns"] = "wrong.test"
            if index == 0 and scenario == "untrusted":
                settings["certificate"] = str(directory / "UNTRUSTED.pem")
                settings["private_key"] = str(directory / "UNTRUSTED.key")
            if index == 1 and scenario == "limit":
                settings["max_bytes"] = 16
            config = directory / (call + ".json")
            config.write_text(json.dumps(dict(encryption=dict(enabled=True, settings=settings))))
            configs.append(config)

        destination = directory / ("received-" + scenario)
        if scenario == "collision":
            destination.write_bytes(b"existing")
        payload = bytes(range(256)) * 2 + b"\x00\xff"
        source = directory / "original.bin"
        source.write_bytes(payload)
        receiver = subprocess.Popen(
            [args.cli, "secure", "receive", configs[1], "-" if scenario == "stdio" else destination],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        stack.callback(stop, receiver)
        time.sleep(0.4)
        sender = subprocess.Popen(
            [args.cli, "secure", "send", configs[0], "-" if scenario == "stdio" else source],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        stack.callback(stop, sender)
        stdout, send_error = sender.communicate(payload if scenario == "stdio" else None, timeout=200)
        assert not stdout
        received, receive_error = receiver.communicate(timeout=200)
        detail = (sender.returncode, receiver.returncode, send_error, receive_error)
        if scenario in ("file", "stdio"):
            assert sender.returncode == receiver.returncode == 0, detail
            actual = received if scenario == "stdio" else destination.read_bytes()
            assert actual == payload
            print(f"{scenario}: SHA-256 {hashlib.sha256(actual).hexdigest()}, both ends disconnected", flush=True)
        else:
            assert sender.returncode != 0 and receiver.returncode != 0, detail
            if scenario == "collision":
                assert destination.read_bytes() == b"existing"
            else:
                assert not destination.exists()
            expected = {
                "pin": b"unexpected secure transfer protocol or peer certificate",
                "dns": b"valid for name",
                "untrusted": b"UnknownIssuer",
                "limit": b"size limit",
                "collision": b"exists",
            }[scenario]
            assert expected.lower() in (send_error + receive_error).lower(), detail
            print(f"{scenario}: rejected without publishing output", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cli")
    parser.add_argument("modem")
    parser.add_argument("profile")
    parser.add_argument("scenarios", nargs="*")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory() as directory:
        directory = Path(directory)
        disabled = directory / "disabled.json"
        disabled.write_text("{}")
        result = subprocess.run([args.cli, "secure", "send", disabled, "-"], capture_output=True, timeout=5)
        assert result.returncode != 0 and b"encryption is disabled" in result.stderr, result
        certificates(directory)
        for scenario in args.scenarios or ("file", "stdio", "pin", "dns", "untrusted", "limit", "collision"):
            test_pair(args, directory, scenario)
