#!/usr/bin/env python3
"""Verify the paired host sockets and audio waveforms between two Linkwise daemons."""
import argparse
import contextlib
import re
import socket
import struct
import subprocess
import time
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


def corrupt_waveform(data):
    count = struct.unpack(">I", data[:4])[0]
    pairs, tail = divmod(count, 2)
    return data[:4] + struct.pack("<ff", 0.2, -0.2) * pairs + struct.pack("<f", 0.2) * tail


def start_station(stack, modem, profile, callsign, bandwidth):
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
    return [int(port) for port in ports]


def connect_station(stack, ports):
    return [
        stack.enter_context(socket.create_connection(("127.0.0.1", port), timeout=8))
        for port in ports
    ]


def attach(stack, modem, profile, callsign, bandwidth):
    return connect_station(stack, start_station(stack, modem, profile, callsign, bandwidth))


def establish_bw500(caller, peer, peer_call="W1AW", caller_call="KN6MQS"):
    caller_cmd, _, caller_audio = caller
    peer_cmd, _, peer_audio = peer
    peer_cmd.sendall(b"LISTEN ON\r")
    assert read_line(peer_cmd) == b"OK\r"
    caller_cmd.sendall(f"CONNECT {caller_call} {peer_call}\r".encode())
    assert read_line(caller_cmd) == b"OK\r"
    for source, destination in [(caller_audio, peer_audio), (peer_audio, caller_audio)] * 3:
        destination.sendall(waveform(source))
    assert read_line(caller_cmd) == read_line(peer_cmd) == f"CONNECTED {caller_call} {peer_call} 500\r".encode()


def responder_sends_on_first_poll(modem, profile, stock_poll=None):
    with contextlib.ExitStack() as stack:
        caller = attach(stack, modem, profile, "W6WW", 500)
        responder = attach(stack, modem, profile, "KN6MQS", 500)
        establish_bw500(caller, responder, "KN6MQS", "W6WW")
        responder[1].sendall(b"Responder greeting")
        assert read_line(responder[0]) == b"BUFFER 18\r"
        time.sleep(0.2)  # The first poll can arrive after the normal batching window.
        caller[1].sendall(b"Caller greeting")
        assert read_line(caller[0]) == b"BUFFER 15\r"
        poll = waveform(caller[2])
        if stock_poll is not None:
            samples = stock_poll.read_bytes()
            poll = struct.pack(">I", len(samples) // 4) + samples
        responder[2].sendall(poll)
        caller[2].sendall(waveform(responder[2]))
        assert read_exact(caller[1], 18) == b"Responder greeting"
        responder[2].sendall(waveform(caller[2]))
        assert read_line(responder[0]) == b"BUFFER 0\r"
    print("BW500 responder delivered queued DATA in response to the first caller poll.")


def replay_stock_rate_down(modem, profile, expected_path, captures, stock_caller=False):
    with contextlib.ExitStack() as stack:
        caller_call, peer_call = ("W6WW", "KN6MQS") if stock_caller else ("KN6MQS", "W6WW")
        caller = attach(stack, modem, profile, caller_call, 500)
        responder = attach(stack, modem, profile, peer_call, 500)
        establish_bw500(caller, responder, peer_call, caller_call)
        sender, receiver = (caller, responder) if stock_caller else (responder, caller)
        sender[1].sendall(b"turn")
        assert read_line(sender[0]) == b"BUFFER 4\r"
        if stock_caller:
            receiver[2].sendall(waveform(sender[2]))
            sender[2].sendall(waveform(receiver[2]))
        receiver[2].sendall(waveform(sender[2]))
        waveform(receiver[2])  # Grant to the stock sender.
        for capture in captures:
            samples = capture.read_bytes()
            receiver[2].sendall(struct.pack(">I", len(samples) // 4) + samples)
            waveform(receiver[2])
        expected = expected_path.read_bytes()
        assert read_exact(receiver[1], len(expected)) == expected
        receiver[1].settimeout(0.2)
        try:
            extra = receiver[1].recv(1)
        except socket.timeout:
            pass
        else:
            raise AssertionError(f"unexpected bytes after stock rate-down replay: {extra!r}")
    print("BW500 stock rate-down replay delivered the original stream without duplication.")


def reconnect_after_channel_loss(modem, profile):
    for lost in range(3):
        with contextlib.ExitStack() as stack:
            ports = start_station(stack, modem, profile, "KN6MQS", 500)
            with contextlib.ExitStack() as old_session:
                old = connect_station(old_session, ports)
                peer = attach(old_session, modem, profile, "W1AW", 500)
                establish_bw500(old, peer)
                old[1].sendall(b"old session")
                assert read_line(old[0]) == b"BUFFER 11\r"
                old[lost].shutdown(socket.SHUT_RDWR)
                for channel, stream in enumerate(old):
                    if channel != lost:
                        while chunk := stream.recv(4096):
                            assert channel != 1, f"stale host DATA: {chunk!r}"

            caller_cmd, caller_data, caller_audio = fresh = connect_station(stack, ports)
            peer = attach(stack, modem, profile, "W1AW", 500)
            _, peer_data, peer_audio = peer
            establish_bw500(fresh, peer)
            payload = b"fresh session"
            caller_data.sendall(payload)
            assert read_line(caller_cmd) == b"BUFFER 13\r"
            for source, destination in [(caller_audio, peer_audio), (peer_audio, caller_audio)] * 3:
                destination.sendall(waveform(source))
            assert read_exact(peer_data, len(payload)) == payload
            assert read_line(caller_cmd) == b"BUFFER 0\r"
    print("Command, DATA, and audio loss each allowed a fresh session without stale bytes.")


def disconnect_after_drain(modem, profile):
    with contextlib.ExitStack() as stack:
        caller_cmd, caller_data, caller_audio = caller = attach(stack, modem, profile, "KN6MQS", 500)
        peer_cmd, peer_data, peer_audio = peer = attach(stack, modem, profile, "W1AW", 500)
        establish_bw500(caller, peer)
        caller_data.sendall(b"FQ\r")
        assert read_line(caller_cmd) == b"BUFFER 3\r"
        caller_cmd.sendall(b"DISCONNECT\r")
        assert read_line(caller_cmd) == b"OK\r"
        for source, destination in [(caller_audio, peer_audio), (peer_audio, caller_audio)] * 3:
            destination.sendall(waveform(source))
        assert read_exact(peer_data, 3) == b"FQ\r"
        assert read_line(caller_cmd) == b"BUFFER 0\r"
        peer_audio.sendall(waveform(caller_audio))
        assert read_line(caller_cmd) == read_line(peer_cmd) == b"DISCONNECTED\r"
    print("Disconnect drained queued DATA and closed at both ends.")


def replay_stock_query_missing_data(modem, profile, capture):
    with contextlib.ExitStack() as stack:
        caller_cmd, caller_data, caller_audio = caller = attach(stack, modem, profile, "KN6MQS", 500)
        _, peer_data, peer_audio = peer = attach(stack, modem, profile, "W1A", 500)
        establish_bw500(caller, peer, "W1A")
        payload = bytes(range(203))
        caller_data.sendall(payload)
        assert read_line(caller_cmd) == b"BUFFER 203\r"
        for source, destination in [(caller_audio, peer_audio), (peer_audio, caller_audio)] * 2:
            destination.sendall(waveform(source))
        for remaining in (160, 117):
            peer_audio.sendall(waveform(caller_audio))
            caller_audio.sendall(waveform(peer_audio))
            assert read_line(caller_cmd) == f"BUFFER {remaining}\r".encode()
        discarded = waveform(caller_audio)
        assert len(waveform(caller_audio)) < len(discarded)
        samples = capture.read_bytes()
        caller_audio.sendall(struct.pack(">I", len(samples) // 4) + samples)
        for remaining in (83, 49, 15, 0):
            peer_audio.sendall(waveform(caller_audio))
            caller_audio.sendall(waveform(peer_audio))
            assert read_line(caller_cmd) == f"BUFFER {remaining}\r".encode()
        assert read_exact(peer_data, len(payload)) == payload
    print("BW500 recovered the missing suffix after 86 acknowledged bytes.")


def replay_crossed_stock_turn(modem, profile, captures):
    with contextlib.ExitStack() as stack:
        caller_cmd, caller_data, caller_audio = caller = attach(stack, modem, profile, "KN6MQS", 500)
        peer_cmd, peer_data, peer_audio = peer = attach(stack, modem, profile, "W1A", 500)
        establish_bw500(caller, peer, "W1A")
        caller_data.sendall(b"probe")
        assert read_line(caller_cmd) == b"BUFFER 5\r"
        for source, destination in [(caller_audio, peer_audio), (peer_audio, caller_audio)] * 3:
            destination.sendall(waveform(source))
        assert read_exact(peer_data, 5) == b"probe"
        assert read_line(caller_cmd) == b"BUFFER 0\r"

        peer_data.sendall(b"continuation")
        assert read_line(peer_cmd) == b"BUFFER 12\r"
        responder_request = waveform(peer_audio)
        caller_audio.sendall(responder_request)
        caller_grant = waveform(caller_audio)
        request, idle, handback, data_nak = (capture.read_bytes() for capture in captures)
        request = struct.pack(">I", len(request) // 4) + request
        peer_audio.sendall(request)
        peer_audio.sendall(request)
        assert len(waveform(peer_audio)) == len(caller_grant)
        peer_audio.sendall(struct.pack(">I", len(idle) // 4) + idle)
        assert waveform(peer_audio) == responder_request
        peer_audio.sendall(struct.pack(">I", len(handback) // 4) + handback)
        first_over = waveform(peer_audio)
        peer_audio.sendall(struct.pack(">I", len(data_nak) // 4) + data_nak)
        retry = waveform(peer_audio)
        assert len(retry) > len(first_over)
        caller_audio.sendall(retry)
        assert read_exact(caller_data, 12) == b"continuation"
        peer_audio.sendall(waveform(caller_audio))
        assert read_line(peer_cmd) == b"BUFFER 0\r"
        waveform(peer_audio)  # Responder releases the turn after its first delivery.
        continuation = b"\x04\xf0"
        peer_data.sendall(continuation)
        assert read_line(peer_cmd) == b"BUFFER 2\r"
        peer_audio.sendall(struct.pack(">I", len(idle) // 4) + idle)
        next_request = waveform(peer_audio)
        assert next_request == responder_request
        caller_audio.sendall(next_request)
        assert waveform(caller_audio) == caller_grant
        peer_audio.sendall(struct.pack(">I", len(handback) // 4) + handback)
        closing_over = waveform(peer_audio)
        assert len(closing_over) == len(retry), "short responder continuation should retain level 3"
        caller_audio.sendall(closing_over)
        assert read_exact(caller_data, 2) == continuation
        peer_audio.sendall(waveform(caller_audio))
        assert read_line(peer_cmd) == b"BUFFER 0\r"
    print("BW500 responder preserved queued DATA through crossed stock turn requests and a DATA rejection.")


def exercise(
    modem, profile, payload, full_frames, corrupt_first=False,
    query_after_loss=False, bandwidth=500, crossed_turns=False, stock_over_nak=None,
    stock_turn_release=None, stock_frame_nak=None, stock_rate_increase=None,
    stock_intermediate_query_ack=None, turn_after_lost_ack=False, corrupt_after_prefix=False,
    query_intermediate=False, stock_wide_nak=None, stock_short_turn_request=None,
    late_write=False, replenish=False,
):
    with contextlib.ExitStack() as stack:
        keyed_capture = stock_rate_increase or stock_intermediate_query_ack or stock_wide_nak or stock_turn_release
        peer_call = "W1A" if keyed_capture else "W1AW"
        caller_cmd, caller_data, caller_audio = attach(stack, modem, profile, "KN6MQS", bandwidth)
        peer_cmd, peer_data, peer_audio = attach(stack, modem, profile, peer_call, bandwidth)
        peer_cmd.sendall(b"LISTEN ON\r")
        assert read_line(peer_cmd) == b"OK\r"
        caller_cmd.sendall(f"CONNECT KN6MQS {peer_call}\r".encode())
        assert read_line(caller_cmd) == b"OK\r"

        request = waveform(caller_audio)
        peer_audio.sendall(request[:2])
        peer_cmd.sendall(b"VERSION\r")
        assert read_line(peer_cmd) == f"VERSION LINKWISE VIRTUAL BW{bandwidth}\r".encode()
        peer_audio.sendall(request[2:])
        for source, destination in [(peer_audio, caller_audio), (caller_audio, peer_audio)] * 2:
            destination.sendall(waveform(source))
        caller_audio.sendall(waveform(peer_audio))
        assert read_line(caller_cmd) == read_line(peer_cmd) == f"CONNECTED KN6MQS {peer_call} {bandwidth}\r".encode()

        caller_data.sendall(payload[:4])
        assert read_line(caller_cmd) == b"BUFFER 4\r"
        caller_data.sendall(payload[4:])
        assert read_line(caller_cmd) == f"BUFFER {len(payload)}\r".encode()
        for index, (source, destination) in enumerate([(caller_audio, peer_audio), (peer_audio, caller_audio)] * 2):
            exchange = waveform(source)
            destination.sendall(exchange)
            if index == 1:
                poll_ack = exchange
        caller_grant = exchange

        data = waveform(caller_audio)
        block_bytes = 43 if bandwidth == 500 else 89
        if corrupt_first:
            peer_audio.sendall(corrupt_waveform(data))
            caller_audio.sendall(waveform(peer_audio))
            retry = waveform(caller_audio)
            assert len(retry) != len(data), "expected a lower-level retry"
            data = retry
            block_bytes = 34
        remaining = len(payload)
        for frame in range(full_frames):
            if frame == 1 and corrupt_after_prefix:
                peer_audio.sendall(corrupt_waveform(data))
                caller_audio.sendall(waveform(peer_audio))
                retry = waveform(caller_audio)
                assert len(retry) != len(data), "expected a lower-level retry after the prefix"
                data = retry
                block_bytes = 34
            peer_audio.sendall(data)
            ack = waveform(peer_audio)
            remaining -= block_bytes
            if frame == int(corrupt_after_prefix) and stock_rate_increase:
                samples = stock_rate_increase.read_bytes()
                ack = struct.pack(">I", len(samples) // 4) + samples
                block_bytes = 43
            if frame == 0 and stock_intermediate_query_ack:
                assert len(waveform(caller_audio)) < len(data), "expected a short recovery query"
                samples = stock_intermediate_query_ack.read_bytes()
                ack = struct.pack(">I", len(samples) // 4) + samples
            if frame == 0 and query_intermediate:
                query = waveform(caller_audio)
                assert len(query) < len(data), "expected a short recovery query"
                peer_audio.sendall(query)
                reply = waveform(peer_audio)
                assert reply != ack, "expected an intermediate state reply"
                ack = reply
            caller_audio.sendall(ack)
            assert read_line(caller_cmd) == f"BUFFER {remaining}\r".encode()
            if replenish:
                caller_data.sendall(bytes(block_bytes))
                assert read_line(caller_cmd) == f"BUFFER {len(payload)}\r".encode()
                return
            data = waveform(caller_audio)

        if stock_frame_nak:
            samples = stock_frame_nak.read_bytes()
            caller_audio.sendall(struct.pack(">I", len(samples) // 4) + samples)
            assert waveform(caller_audio) == data, "retry must keep the acknowledged prefix"
        if stock_wide_nak:
            samples = stock_wide_nak.read_bytes()
            caller_audio.sendall(struct.pack(">I", len(samples) // 4) + samples)
            retry = waveform(caller_audio)
            assert len(retry) > len(data), "expected the outstanding closing frame at level 3"
            data = retry

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
            caller_audio.settimeout(15)
            query = waveform(caller_audio)
            caller_audio.settimeout(8)
            assert query != data, "missing ACK must solicit state before repeating DATA"
            if turn_after_lost_ack:
                reply = b"F> 79\r"
                peer_data.sendall(reply)
                assert read_line(peer_cmd) == b"BUFFER 6\r"
                caller_audio.sendall(waveform(peer_audio))
                assert read_line(caller_cmd) == b"BUFFER 0\r"
                peer_audio.sendall(waveform(caller_audio))
                caller_audio.sendall(waveform(peer_audio))
                assert read_exact(caller_data, len(reply)) == reply
                peer_audio.sendall(waveform(caller_audio))
                assert read_line(peer_cmd) == b"BUFFER 0\r"
                print("BW500 caller accepted direct DATA after a turn request answered its final query.")
                return
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

        if late_write:
            continuation = b"\x01\x00"
            caller_data.sendall(continuation)
            assert read_line(caller_cmd) == b"BUFFER 2\r"
            peer_audio.sendall(waveform(caller_audio))
            caller_audio.sendall(waveform(peer_audio))
            burst = waveform(caller_audio)
            assert len(burst) == 4 + 235_520 * 4, "short continuation should retain BW500 level 3"
            peer_data.settimeout(8)
            peer_audio.sendall(burst)
            assert read_exact(peer_data, len(continuation)) == continuation
            caller_audio.sendall(waveform(peer_audio))
            assert read_line(caller_cmd) == b"BUFFER 0\r"

        if stock_turn_release:
            samples = stock_turn_release.read_bytes()
            peer_audio.sendall(struct.pack(">I", len(samples) // 4) + samples)
            assert waveform(peer_audio) == poll_ack
            if stock_short_turn_request:
                samples = stock_short_turn_request.read_bytes()
                peer_audio.sendall(struct.pack(">I", len(samples) // 4) + samples)
                assert waveform(peer_audio) == caller_grant
            next_payload = payload
            caller_data.sendall(next_payload)
            assert read_line(caller_cmd) == f"BUFFER {len(next_payload)}\r".encode()
            waveform(caller_audio)  # Stock caller would use the returned turn directly.
            caller_audio.sendall(caller_grant)
            peer_data.settimeout(15)
            repeated = waveform(caller_audio)
            assert repeated == data, "expected the same closing frame on the returned turn"
            peer_audio.sendall(repeated)
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
        if bandwidth == 500:
            release = waveform(peer_audio)
            caller_audio.sendall(release)
            waveform(caller_audio)  # Empty caller acknowledges the returned turn.

        if crossed_turns:
            direct = b"DATA on the returned turn"
            caller_data.sendall(direct)
            assert read_line(caller_cmd) == f"BUFFER {len(direct)}\r".encode()
            waveform(caller_audio)  # Stock caller can skip the request after release.
            caller_audio.sendall(caller_grant)
            peer_data.settimeout(8)
            peer_audio.sendall(waveform(caller_audio))
            assert read_exact(peer_data, len(direct)) == direct
            caller_audio.sendall(waveform(peer_audio))
            assert read_line(caller_cmd) == b"BUFFER 0\r"

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
            waveform(peer_audio)  # The responder releases the turn after its ACK.
            for source, destination in [(caller_audio, peer_audio), (peer_audio, caller_audio)] * 2:
                destination.sendall(waveform(source))
            assert read_exact(peer_data, 6) == b"caller"
            assert read_line(caller_cmd) == b"BUFFER 0\r"

        caller_cmd.sendall(b"ABORT\r")
        assert read_line(caller_cmd) == b"OK\r"
        assert read_line(caller_cmd) == b"DISCONNECTED\r"
    print(f"BW{bandwidth} bidirectional delivery, split TCP input, and closing-frame recovery passed.")


def replay_stock_greeting(modem, profile, captures, peer_call="W1AW"):
    with contextlib.ExitStack() as stack:
        caller_cmd, caller_data, caller_audio = attach(stack, modem, profile, "KN6MQS", 500)
        peer_cmd, _, peer_audio = attach(stack, modem, profile, peer_call, 500)
        peer_cmd.sendall(b"LISTEN ON\r")
        assert read_line(peer_cmd) == b"OK\r"
        caller_cmd.sendall(f"CONNECT KN6MQS {peer_call}\r".encode())
        assert read_line(caller_cmd) == b"OK\r"
        for source, destination in [(caller_audio, peer_audio), (peer_audio, caller_audio)] * 2:
            destination.sendall(waveform(source))
        peer_audio.sendall(waveform(caller_audio))
        waveform(peer_audio)  # Stock VARA sends DATA in place of this confirmation ACK.
        for capture in captures:
            samples = Path(capture).read_bytes()
            caller_audio.sendall(struct.pack(">I", len(samples) // 4) + samples)
            waveform(caller_audio)
        assert read_line(caller_cmd) == f"CONNECTED KN6MQS {peer_call} 500\r".encode()
        greeting = f";FW: {peer_call}\r[Pat-1.0.1-B2FHMG$]\r; KN6MQS DE {peer_call} ()>\r".encode()
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
        disconnect = waveform(caller_audio)
        assert len(disconnect) < len(data), "silence must not trigger DATA retransmission"
        assert read_line(caller_cmd) == b"DISCONNECTED\r", "silence must leave delivery unconfirmed"
        peer_audio.sendall(disconnect)
        assert read_line(peer_cmd) == b"DISCONNECTED\r"
        establish_bw500(
            (caller_cmd, caller_data, caller_audio), (peer_cmd, peer_data, peer_audio)
        )
    print("Consecutive BW500 deliveries recovered an intermediate ACK without duplicate bytes.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("modem", help="path to linkwise-modemd")
    parser.add_argument("profile", help="path to the measured BW500 level-4 profile")
    parser.add_argument("--wide", action="store_true", help="also check BW2300 and BW2750")
    parser.add_argument("--stock-greeting", nargs=2, metavar="F32", help="replay the captured full and closing greeting frames")
    parser.add_argument("--stock-greeting-call", default="W1AW", help="called station in the captured greeting")
    parser.add_argument("--stock-over-nak", type=Path, metavar="F32", help="replay the captured stock over rejection")
    parser.add_argument("--stock-turn-release", type=Path, metavar="F32", help="replay the captured stock caller release")
    parser.add_argument("--stock-short-turn-request", type=Path, metavar="F32", help="replay KN6MQS's short turn request to W1A")
    parser.add_argument("--stock-frame-nak", type=Path, metavar="F32", help="replay the captured stock frame rejection")
    parser.add_argument("--stock-rate-increase", type=Path, metavar="F32", help="replay W1A's captured rate-increase acknowledgment")
    parser.add_argument("--stock-intermediate-query-ack", type=Path, metavar="F32", help="replay W1A's response to a short intermediate query")
    parser.add_argument("--stock-query-missing-data", type=Path, metavar="F32", help="replay W1A's BW500 query response for a missing intermediate frame")
    parser.add_argument("--stock-crossed-turn", nargs=4, type=Path, metavar="F32",
                        help="replay the stock caller's request, idle, handback, and DATA rejection")
    parser.add_argument("--stock-wide-nak", type=Path, metavar="F32", help="replay W1A's BW2750 negative acknowledgment")
    parser.add_argument("--stock-caller-poll", type=Path, metavar="F32",
                        help="replay W6WW's BW500 post-connect poll to KN6MQS")
    parser.add_argument("--stock-responder-rate-down", nargs="+", type=Path, metavar="FILE",
                        help="expected host bytes followed by captured stock responder DATA bursts")
    parser.add_argument("--stock-caller-rate-down", nargs="+", type=Path, metavar="FILE",
                        help="expected host bytes followed by captured stock caller DATA bursts")
    args = parser.parse_args()
    if args.stock_short_turn_request and not args.stock_turn_release:
        parser.error("--stock-short-turn-request requires --stock-turn-release")
    exercise(
        args.modem, args.profile, b"\x00\xff\x01Linkwise modem\x14\x00", 0,
        corrupt_first=True, query_after_loss=True,
    )
    exercise(args.modem, args.profile, bytes(range(200)), 4, crossed_turns=True)
    exercise(args.modem, args.profile, bytes(range(84)), 1)
    exercise(args.modem, args.profile, bytes(range(164)), 3)
    exercise(args.modem, args.profile, bytes(4096), 4, replenish=True)
    exercise(args.modem, args.profile, bytes(range(200)), 5, corrupt_first=True)
    exercise(
        args.modem, args.profile, b"first over", 0,
        query_after_loss=True, turn_after_lost_ack=True,
    )
    exercise(args.modem, args.profile, bytes(range(200)), 4, query_intermediate=True, late_write=True)
    recover_intermediate_ack(args.modem, args.profile, args.stock_over_nak)
    reconnect_after_channel_loss(args.modem, args.profile)
    disconnect_after_drain(args.modem, args.profile)
    responder_sends_on_first_poll(args.modem, args.profile, args.stock_caller_poll)
    for stock_caller, fixture in ((False, args.stock_responder_rate_down),
                                  (True, args.stock_caller_rate_down)):
        if fixture:
            expected, *captures = fixture
            if not captures:
                parser.error("a stock rate-down replay requires at least one captured DATA burst")
            replay_stock_rate_down(args.modem, args.profile, expected, captures, stock_caller)
    if args.wide:
        for bandwidth in (2300, 2750):
            exercise(args.modem, args.profile, bytes(range(86)), 0, bandwidth=bandwidth, query_after_loss=True)
            exercise(args.modem, args.profile, bytes(range(203)), 2, bandwidth=bandwidth, query_after_loss=True)
    if args.stock_greeting:
        replay_stock_greeting(args.modem, args.profile, args.stock_greeting, args.stock_greeting_call)
    if args.stock_over_nak:
        exercise(args.modem, args.profile, b"retry the whole over", 0, stock_over_nak=args.stock_over_nak)
    if args.stock_turn_release:
        exercise(
            args.modem, args.profile, b"first over", 0, stock_turn_release=args.stock_turn_release,
            stock_short_turn_request=args.stock_short_turn_request,
        )
    if args.stock_frame_nak:
        exercise(args.modem, args.profile, bytes(range(84)), 1, stock_frame_nak=args.stock_frame_nak)
    if args.stock_rate_increase:
        exercise(
            args.modem, args.profile, bytes(range(200)), 4,
            corrupt_first=True, stock_rate_increase=args.stock_rate_increase,
        )
        exercise(
            args.modem, args.profile, bytes(range(200)), 4,
            corrupt_after_prefix=True, stock_rate_increase=args.stock_rate_increase,
        )
    if args.stock_intermediate_query_ack:
        exercise(
            args.modem, args.profile, bytes(range(200)), 4,
            stock_intermediate_query_ack=args.stock_intermediate_query_ack,
        )
    if args.stock_query_missing_data:
        replay_stock_query_missing_data(args.modem, args.profile, args.stock_query_missing_data)
    if args.stock_crossed_turn:
        replay_crossed_stock_turn(args.modem, args.profile, args.stock_crossed_turn)
    if args.stock_wide_nak:
        exercise(
            args.modem, args.profile, bytes(range(203)), 2,
            bandwidth=2750, stock_wide_nak=args.stock_wide_nak,
        )
