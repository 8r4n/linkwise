#!/usr/bin/env python3
"""Verify the paired host sockets and audio waveforms between two Linkwise daemons."""
import argparse
import contextlib
import re
import socket
import struct
import subprocess
from pathlib import Path


def read_exact(stream, count):
    result = bytearray()
    while len(result) < count:
        chunk = stream.recv(count - len(result))
        if not chunk:
            raise EOFError("Linkwise closed the connection")
        result.extend(chunk)
    return bytes(result)


def read_line(stream):
    line = bytearray()
    while not line.endswith(b"\r"):
        line.extend(read_exact(stream, 1))
    return bytes(line)


def waveform(stream):
    header = read_exact(stream, 4)
    samples = struct.unpack(">I", header)[0]
    assert 1 <= samples <= 288_000
    return header + read_exact(stream, samples * 4)


def attach(stack, modem, profile, callsign, bandwidth):
    mode, setting = (
        ("--serve-virtual-bw500", profile)
        if bandwidth == 500 else ("--serve-virtual-wide", str(bandwidth))
    )
    process = subprocess.Popen(
        [modem, mode, callsign, setting, "0", "0", "0"],
        stdout=subprocess.PIPE,
        text=True,
    )

    def stop():
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        process.stdout.close()

    stack.callback(stop)
    ports = re.findall(r"127\.0\.0\.1:(\d+)", process.stdout.readline())
    if len(ports) != 3:
        raise RuntimeError("the virtual modem did not advertise all three ports")
    return [
        stack.enter_context(socket.create_connection(("127.0.0.1", int(port)), timeout=8))
        for port in ports
    ]


def exercise(
    modem, profile, payload, full_frames, corrupt_first=False,
    query_after_loss=False, bandwidth=500, crossed_turns=False, stock_over_nak=None,
    stock_turn_release=None, stock_frame_nak=None,
):
    with contextlib.ExitStack() as stack:
        caller_cmd, caller_data, caller_audio = attach(stack, modem, profile, "KN6MQS", bandwidth)
        peer_cmd, peer_data, peer_audio = attach(stack, modem, profile, "W1AW", bandwidth)
        peer_cmd.sendall(b"LISTEN ON\r")
        assert read_line(peer_cmd) == b"OK\r"
        caller_cmd.sendall(b"CONNECT KN6MQS W1AW\r")
        assert read_line(caller_cmd) == b"OK\r"

        request = waveform(caller_audio)
        peer_audio.sendall(request[:2])
        peer_cmd.sendall(b"VERSION\r")
        assert read_line(peer_cmd) == f"VERSION LINKWISE VIRTUAL BW{bandwidth}\r".encode()
        peer_audio.sendall(request[2:])
        for source, destination in [(peer_audio, caller_audio), (caller_audio, peer_audio)] * 2:
            destination.sendall(waveform(source))
        caller_audio.sendall(waveform(peer_audio))
        assert read_line(caller_cmd) == read_line(peer_cmd) == f"CONNECTED KN6MQS W1AW {bandwidth}\r".encode()

        caller_data.sendall(payload[:4])
        assert read_line(caller_cmd) == b"BUFFER 4\r"
        caller_data.sendall(payload[4:])
        assert read_line(caller_cmd) == f"BUFFER {len(payload)}\r".encode()
        for source, destination in [(caller_audio, peer_audio), (peer_audio, caller_audio)] * 2:
            exchange = waveform(source)
            destination.sendall(exchange)
        caller_grant = exchange

        data = waveform(caller_audio)
        block_bytes = 43
        if corrupt_first:
            count = struct.unpack(">I", data[:4])[0]
            corrupted = struct.pack("<ff", 0.2, -0.2) * (count // 2)
            if count % 2:
                corrupted += struct.pack("<f", 0.2)
            peer_audio.sendall(data[:4] + corrupted)
            caller_audio.sendall(waveform(peer_audio))
            retry = waveform(caller_audio)
            assert len(retry) != len(data), "expected a lower-level retry"
            data = retry
            block_bytes = 34
        for frame in range(full_frames):
            peer_audio.sendall(data)
            caller_audio.sendall(waveform(peer_audio))
            remaining = len(payload) - (frame + 1) * block_bytes
            assert read_line(caller_cmd) == f"BUFFER {remaining}\r".encode()
            data = waveform(caller_audio)

        if stock_frame_nak:
            samples = stock_frame_nak.read_bytes()
            caller_audio.sendall(struct.pack(">I", len(samples) // 4) + samples)
            assert waveform(caller_audio) == data, "retry must keep the acknowledged prefix"

        if stock_over_nak:
            # The peer misses the complete first over. Answer the caller's
            # recovery query with the captured stock VARA negative reply.
            assert waveform(caller_audio) != data
            samples = stock_over_nak.read_bytes()
            caller_audio.sendall(struct.pack(">I", len(samples) // 4) + samples)
            retry = waveform(caller_audio)
            assert len(retry) != len(data), "expected a lower-level retry after the over NAK"
            data = retry

        peer_audio.sendall(data)
        lost_ack = waveform(peer_audio)
        assert read_exact(peer_data, len(payload)) == payload

        if query_after_loss:
            query = waveform(caller_audio)
            assert query != data, "missing ACK must solicit state before repeating DATA"
            peer_audio.sendall(query)
        else:
            # An exact repeated closing frame may be acknowledged again.
            peer_audio.sendall(data)
        recovered_ack = waveform(peer_audio)
        assert recovered_ack == lost_ack
        peer_data.settimeout(0.2)
        try:
            extra = peer_data.recv(1)
        except socket.timeout:
            pass
        else:
            raise AssertionError(f"unexpected second delivery or socket close: {extra!r}")
        caller_audio.sendall(recovered_ack)
        assert read_line(caller_cmd) == b"BUFFER 0\r"

        if stock_turn_release:
            samples = stock_turn_release.read_bytes()
            peer_audio.sendall(struct.pack(">I", len(samples) // 4) + samples)
            waveform(peer_audio)  # Receiver hands the empty turn back to the caller.
            next_payload = b"direct DATA after handback"
            caller_data.sendall(next_payload)
            assert read_line(caller_cmd) == f"BUFFER {len(next_payload)}\r".encode()
            waveform(caller_audio)  # Stock caller would use the returned turn directly.
            caller_audio.sendall(caller_grant)
            peer_data.settimeout(15)
            peer_audio.sendall(waveform(caller_audio))
            assert read_exact(peer_data, len(next_payload)) == next_payload
            caller_audio.sendall(waveform(peer_audio))
            assert read_line(caller_cmd) == b"BUFFER 0\r"

        response = b"answer\x00\xff"
        peer_data.sendall(response)
        assert read_line(peer_cmd) == f"BUFFER {len(response)}\r".encode()
        for source, destination in [(peer_audio, caller_audio), (caller_audio, peer_audio)] * 2:
            destination.sendall(waveform(source))
        assert read_exact(caller_data, len(response)) == response
        assert read_line(peer_cmd) == b"BUFFER 0\r"

        if crossed_turns:
            caller_data.sendall(b"caller")
            peer_data.sendall(b"peer")
            assert read_line(caller_cmd) == b"BUFFER 6\r"
            assert read_line(peer_cmd) == b"BUFFER 4\r"
            caller_request, peer_request = waveform(caller_audio), waveform(peer_audio)
            peer_audio.sendall(caller_request)
            caller_audio.sendall(peer_request)
            for source, destination in [(caller_audio, peer_audio), (peer_audio, caller_audio)]:
                destination.sendall(waveform(source))
            assert read_exact(caller_data, 4) == b"peer"
            peer_audio.sendall(waveform(caller_audio))
            assert read_line(peer_cmd) == b"BUFFER 0\r"
            for source, destination in [(caller_audio, peer_audio), (peer_audio, caller_audio)] * 2:
                destination.sendall(waveform(source))
            assert read_exact(peer_data, 6) == b"caller"
            assert read_line(caller_cmd) == b"BUFFER 0\r"

        caller_cmd.sendall(b"ABORT\r")
        assert read_line(caller_cmd) == b"OK\r"
        assert read_line(caller_cmd) == b"DISCONNECTED\r"
    print(f"BW{bandwidth} bidirectional delivery, split TCP input, and closing-frame recovery passed.")


def replay_stock_greeting(modem, profile, captures):
    with contextlib.ExitStack() as stack:
        caller_cmd, caller_data, caller_audio = attach(stack, modem, profile, "KN6MQS", 500)
        peer_cmd, _, peer_audio = attach(stack, modem, profile, "W1AW", 500)
        peer_cmd.sendall(b"LISTEN ON\r")
        assert read_line(peer_cmd) == b"OK\r"
        caller_cmd.sendall(b"CONNECT KN6MQS W1AW\r")
        assert read_line(caller_cmd) == b"OK\r"
        for source, destination in [(caller_audio, peer_audio), (peer_audio, caller_audio)] * 2:
            destination.sendall(waveform(source))
        peer_audio.sendall(waveform(caller_audio))
        waveform(peer_audio)  # Stock VARA sends DATA in place of this confirmation ACK.
        for capture in captures:
            samples = Path(capture).read_bytes()
            caller_audio.sendall(struct.pack(">I", len(samples) // 4) + samples)
            waveform(caller_audio)
        assert read_line(caller_cmd) == b"CONNECTED KN6MQS W1AW 500\r"
        greeting = b";FW: W1AW\r[Pat-1.0.1-B2FHMG$]\r; KN6MQS DE W1AW ()>\r"
        assert read_exact(caller_data, len(greeting)) == greeting
    print("Stock BW500 greeting before the confirmation ACK replayed exactly.")


def recover_intermediate_ack(modem, profile, stock_over_nak):
    with contextlib.ExitStack() as stack:
        caller_cmd, caller_data, caller_audio = attach(stack, modem, profile, "KN6MQS", 500)
        peer_cmd, peer_data, peer_audio = attach(stack, modem, profile, "W1AW", 500)
        peer_cmd.sendall(b"LISTEN ON\r")
        assert read_line(peer_cmd) == b"OK\r"
        caller_cmd.sendall(b"CONNECT KN6MQS W1AW\r")
        assert read_line(caller_cmd) == b"OK\r"
        for source, destination in [(caller_audio, peer_audio), (peer_audio, caller_audio)] * 2:
            destination.sendall(waveform(source))
        peer_audio.sendall(waveform(caller_audio))
        caller_audio.sendall(waveform(peer_audio))
        assert read_line(caller_cmd) == read_line(peer_cmd) == b"CONNECTED KN6MQS W1AW 500\r"

        first = bytes(range(203))
        second = bytes(reversed(first))
        caller_data.sendall(first + second)
        remaining = len(first + second)
        assert read_line(caller_cmd) == f"BUFFER {remaining}\r".encode()
        for source, destination in [(caller_audio, peer_audio), (peer_audio, caller_audio)]:
            destination.sendall(waveform(source))
        for delivery, payload in enumerate((first, second)):
            peer_audio.sendall(waveform(caller_audio))
            caller_audio.sendall(waveform(peer_audio))
            for frame in range(5):
                data = waveform(caller_audio)
                if delivery == 1 and frame == 2 and stock_over_nak:
                    # No DATA reaches the receiver. A negative query answer
                    # must replay this frame alone, with the prefix intact.
                    assert len(waveform(caller_audio)) < len(data)
                    samples = stock_over_nak.read_bytes()
                    caller_audio.sendall(struct.pack(">I", len(samples) // 4) + samples)
                    assert waveform(caller_audio) == data
                peer_audio.sendall(data)
                ack = waveform(peer_audio)
                if delivery == 1 and frame == 0:
                    # Withhold the intermediate ACK through both queries,
                    # then deliver it while the frame is still outstanding.
                    short = waveform(caller_audio)
                    long = waveform(caller_audio)
                    assert len(short) < len(long) < len(data)
                caller_audio.sendall(ack)
                remaining -= 43 if frame < 4 else 31
                assert read_line(caller_cmd) == f"BUFFER {remaining}\r".encode()
            assert read_exact(peer_data, len(payload)) == payload
        peer_data.settimeout(0.2)
        try:
            duplicate = peer_data.recv(1)
        except socket.timeout:
            pass
        else:
            raise AssertionError(f"unexpected bytes after second delivery: {duplicate!r}")
        caller_data.sendall(bytes(range(84)))
        assert read_line(caller_cmd) == b"BUFFER 84\r"
        peer_audio.sendall(waveform(caller_audio))
        caller_audio.sendall(waveform(peer_audio))
        data = waveform(caller_audio)
        assert len(waveform(caller_audio)) < len(data)
        assert len(waveform(caller_audio)) < len(data)
        assert caller_audio.recv(1) == b"", "silence must not trigger DATA retransmission"
    print("Consecutive BW500 deliveries recovered an intermediate ACK without duplicate bytes.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("modem", help="path to linkwise-modemd")
    parser.add_argument("profile", help="path to the measured BW500 level-4 profile")
    parser.add_argument("--wide", action="store_true", help="also check BW2300 and BW2750")
    parser.add_argument("--stock-greeting", nargs=2, metavar="F32", help="replay the captured full and closing greeting frames")
    parser.add_argument("--stock-over-nak", type=Path, metavar="F32", help="replay the captured stock over rejection")
    parser.add_argument("--stock-turn-release", type=Path, metavar="F32", help="replay the captured stock caller release")
    parser.add_argument("--stock-frame-nak", type=Path, metavar="F32", help="replay the captured stock frame rejection")
    args = parser.parse_args()
    exercise(
        args.modem, args.profile, b"\x00\xff\x01Linkwise modem\x14\x00", 0,
        corrupt_first=True, query_after_loss=True,
    )
    exercise(args.modem, args.profile, bytes(range(200)), 4, crossed_turns=True)
    exercise(args.modem, args.profile, bytes(range(84)), 1)
    exercise(args.modem, args.profile, bytes(range(200)), 5, corrupt_first=True)
    recover_intermediate_ack(args.modem, args.profile, args.stock_over_nak)
    if args.wide:
        for bandwidth in (2300, 2750):
            exercise(args.modem, args.profile, bytes(range(86)), 0, bandwidth=bandwidth)
    if args.stock_greeting:
        replay_stock_greeting(args.modem, args.profile, args.stock_greeting)
    if args.stock_over_nak:
        exercise(args.modem, args.profile, b"retry the whole over", 0, stock_over_nak=args.stock_over_nak)
    if args.stock_turn_release:
        exercise(args.modem, args.profile, b"first over", 0, stock_turn_release=args.stock_turn_release)
    if args.stock_frame_nak:
        exercise(args.modem, args.profile, bytes(range(84)), 1, stock_frame_nak=args.stock_frame_nak)
