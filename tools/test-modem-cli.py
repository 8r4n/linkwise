#!/usr/bin/env python3
"""Check binary CLI streams using the supplied Linkwise executable."""
import argparse
import json
import os
from pathlib import Path
import select
import signal
import socket
import subprocess
import tempfile


def expect_bytes(connection, expected):
    received = bytearray()
    while len(received) < len(expected):
        chunk = connection.recv(len(expected) - len(received))
        assert chunk, f"Modem socket closed after receiving {received!r}"
        received.extend(chunk)
    assert received == expected, f"Expected {expected!r}, received {received!r}"


def expect_waiting(process, *connections):
    readable, _, _ = select.select(connections, [], [], 0.1)
    assert not readable, "CLI sent more data, issued a command, or closed before acknowledgment"
    assert process.poll() is None, "CLI exited before acknowledgment"


def check_send(cli, outcome, stop_signal=None):
    # Use distinct bytes in each chunk so a replay or reorder fails the check.
    chunks = (
        bytes(range(256)) * 4,
        bytes(reversed(range(256))) * 4,
        b"final\x00\xffchunk",
    )
    with socket.socket() as command_listener, socket.socket() as data_listener:
        for listener in (command_listener, data_listener):
            listener.bind(("127.0.0.1", 0))
            listener.listen()
            listener.settimeout(5)
        ports = [str(listener.getsockname()[1]) for listener in (command_listener, data_listener)]
        with subprocess.Popen(
            [cli, "send", "W1AW", "K1ABC", "-", *ports],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        ) as process:
            try:
                if outcome == "blocked-input":
                    chunks = chunks[:1]
                process.stdin.write(b"".join(chunks))
                process.stdin.flush()
                if outcome != "blocked-input":
                    process.stdin.close()
                    process.stdin = None
                with command_listener.accept()[0] as command, data_listener.accept()[0] as data:
                    for connection in (command, data):
                        connection.settimeout(5)
                        connection.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
                    for request in (b"MYCALL W1AW\r", b"P2P SESSION\r", b"CONNECT W1AW K1ABC\r"):
                        expect_bytes(command, request)
                        command.sendall(b"OK\r")
                    command.sendall(b"CONNECTED W1AW K1ABC 500\r")

                    for chunk in chunks:
                        expect_bytes(data, chunk)
                        command.sendall(f"BUFFER {len(chunk)}\rBUFFER 1\r".encode("ascii"))
                        expect_waiting(process, command, data)
                        if outcome == "disconnect":
                            command.sendall(b"DISCONNECTED\r")
                            break
                        if outcome == "interrupt":
                            process.send_signal(stop_signal)
                            break
                        command.sendall(b"BUF")
                        expect_waiting(process, command, data)
                        command.sendall(b"FER 0\r")

                    if outcome in ("success", "closing"):
                        expect_bytes(command, b"DISCONNECT\r")
                        command.sendall(b"OK\r")
                        expect_waiting(process, command, data)
                        if outcome == "closing":
                            process.send_signal(stop_signal)
                        else:
                            command.sendall(b"DISCONNECTED\r")
                    if outcome == "blocked-input":
                        expect_waiting(process, command, data)
                        process.send_signal(stop_signal)
                        process.wait(timeout=5)
                        process.stdin.close()
                        process.stdin = None

                    stdout, stderr = process.communicate(timeout=5)
                    assert not stdout, stdout
                    if outcome == "disconnect":
                        assert process.returncode == 1 and b"unconfirmed bytes" in stderr, stderr
                    elif stop_signal is not None:
                        assert process.returncode == 128 + stop_signal, (process.returncode, stderr)
                        assert b"transmission interrupted" in stderr, stderr
                    else:
                        assert process.returncode == 0 and not stderr, stderr
                    assert data.recv(1) == b"", "Unexpected trailing payload byte"
                    assert command.recv(1) == b"", "Unexpected trailing modem command"
            finally:
                if process.poll() is None:
                    process.kill()


def check(cli, directory):
    def run(*args, data=None):
        return subprocess.run([cli, *args], input=data, capture_output=True, timeout=30)

    source = directory / "payload.bin"
    wav = directory / "tx.wav"
    for level, size in ((1, 9), (2, 22), (3, 34)):
        payload = bytes(range(size))
        source.write_bytes(payload)
        rendered = run("encode-frame", str(level), "0xa5", source, wav)
        assert (rendered.returncode, rendered.stdout, rendered.stderr) == (0, b"", b""), rendered
        piped = run("encode-frame", str(level), "165", "-", "-", data=payload)
        assert piped.returncode == 0 and piped.stderr == b"", piped.stderr
        assert piped.stdout == wav.read_bytes()
        decoded = run("decode-frame", "-", data=piped.stdout)
        assert (decoded.returncode, decoded.stdout, decoded.stderr) == (0, payload, b""), decoded
        report = run("decode-wav", wav)
        assert report.returncode == 0 and report.stderr == b"", report.stderr
        frame, = json.loads(report.stdout)["bw500_data_frames"]
        assert (frame["level"], frame["control"], frame["payload_hex"]) == (level, 165, payload.hex())

    pcm = run("encode-frame", "1", "0", "-", "-", "pcm-f32le", data=bytes(9))
    assert pcm.returncode == 0 and len(pcm.stdout) % 4 == 0 and pcm.stdout[:4] != b"RIFF"
    for bad in (b"", bytes(8), bytes(10)):
        rejected = run("encode-frame", "1", "0", "-", "-", data=bad)
        assert rejected.returncode == 1 and not rejected.stdout and rejected.stderr
    assert run("encode-frame", "1", "256", "-", "-", data=bytes(9)).returncode == 2
    assert run("send", "W1AW", "K1ABC", "-", data=b"").returncode == 0
    check_send(cli, "success")
    check_send(cli, "disconnect")
    if os.name == "posix":
        check_send(cli, "interrupt", signal.SIGINT)
        for phase in ("blocked-input", "interrupt", "closing"):
            check_send(cli, phase, signal.SIGTERM)

    source.write_bytes(bytes(9))
    with subprocess.Popen(
        [cli, "encode-frame", "1", "0", source, "-"],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    ) as process:
        process.stdout.close()
        assert process.wait(timeout=10) == 1 and process.stderr.read() == b""


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cli", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory() as directory:
        check(args.cli.resolve(), Path(directory))
    print("Binary modem CLI checks passed")
