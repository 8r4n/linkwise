#!/usr/bin/env python3
"""Drive a radio-free VARA interoperability fixture over guest audio bridges."""

import argparse
import array
from collections import deque
from contextlib import ExitStack
import hashlib
import json
from pathlib import Path
import queue
import re
import socket
import struct
import subprocess
import threading
import time
import wave


SAMPLE_RATE = 48_000
MAX_RECORD = 6 * SAMPLE_RATE
PCM_WINDOW = SAMPLE_RATE // 100
PCM_TICK = SAMPLE_RATE // 50
CALLSIGNS = ("KN6MQS", "W1A")
BASE_PORTS = (28760, 28770)
HF_PROFILES = {32: (1, 0.5, -9, 17), 22: (2, 1, -4, 18)}
RETURN_PAYLOAD = b"F> 79\r"


class BenchFault(Exception):
    """The bench could not establish or maintain the test transport."""


class FixtureFailure(Exception):
    """The fixture did not meet its acceptance criteria."""


def read_exact(stream, size):
    data = bytearray()
    while len(data) < size:
        chunk = stream.recv(size - len(data))
        if not chunk:
            raise EOFError
        data.extend(chunk)
    return bytes(data)


def read_pcm(stream):
    count, = struct.unpack(">I", read_exact(stream, 4))
    if not 1 <= count <= MAX_RECORD:
        raise BenchFault(f"invalid audio record length {count}")
    return read_exact(stream, count * 4)


def write_pcm(stream, pcm):
    stream.sendall(struct.pack(">I", len(pcm) // 4) + pcm)


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def file_sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_replacement(path):
    with wave.open(str(path), "rb") as source:
        channels = source.getnchannels()
        if channels not in (1, 2) or source.getsampwidth() != 2 or source.getframerate() != SAMPLE_RATE:
            raise ValueError("replacement must be a mono or stereo 48 kHz PCM16 WAV")
        samples = array.array("h", source.readframes(source.getnframes()))
    return array.array("f", (sample / 32768 for sample in samples[::channels])).tobytes()


def stop(process):
    if process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=3)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()


class Trial:
    def __init__(self, args, transcript):
        self.args = args
        self.transcript = transcript
        self.start = time.monotonic()
        self.changed = threading.Condition()
        self.log_lock = threading.Lock()
        self.commands = [bytearray(), bytearray()]
        self.received = [bytearray(), bytearray()]
        self.closed = set()
        self.transport_fault = None
        self.disconnected = [False, False]
        self.measurements = {}
        self.fault_injected = False
        self.stock_retry_seen = False
        self.rate_reply_injected = False
        self.phase = "bridge setup"
        self.tearing_down = False
        self.hf_workers = {}
        self.expected_streams = None

    def log(self, *items):
        with self.log_lock:
            line = f"{time.monotonic() - self.start:8.3f} " + " ".join(map(str, items))
            print(line, flush=True)
            print(line, file=self.transcript, flush=True)

    def start_linkwise(self, stack, role):
        mode = (
            ["--serve-virtual-bw500", CALLSIGNS[role], str(self.args.profile)]
            if self.args.bandwidth == 500
            else ["--serve-virtual-wide", CALLSIGNS[role], str(self.args.bandwidth)]
        )
        stderr = stack.enter_context((self.args.output / f"linkwise-{role}.stderr").open("w"))
        process = subprocess.Popen(
            [str(self.args.modem), *mode, "0", "0", "0"],
            stdout=subprocess.PIPE, stderr=stderr, text=True,
        )
        stack.callback(process.stdout.close)
        stack.callback(stop, process)
        startup = process.stdout.readline()
        ports = re.findall(r"127\.0\.0\.1:(\d+)", startup)
        if len(ports) != 3:
            raise BenchFault(f"Linkwise did not expose its three sockets: {startup!r}")
        return [
            stack.enter_context(socket.create_connection(("127.0.0.1", int(port))))
            for port in ports
        ]

    def record_stream(self, source, role):
        name = ("caller", "responder")[role]
        with (self.args.output / f"{name}-stream.f32").open("wb") as audio, (
            self.args.output / f"{name}-chunks.tsv"
        ).open("w") as timing:
            while True:
                pcm = read_pcm(source)
                audio.write(pcm)
                print(f"{time.monotonic() - self.start:.6f}\t{len(pcm) // 4}", file=timing)
                yield pcm

    def save_burst(self, role, index, pcm):
        name = ("caller", "responder")[role]
        (self.args.output / f"{name}-{index:02}.f32").write_bytes(pcm)
        self.log(name, "burst", index, len(pcm) // 4)

    def impair(self, pcm, role):
        if role not in self.hf_workers:
            return pcm
        process = self.hf_workers[role]
        header = struct.pack(">I", len(pcm) // 4)
        process.stdin.write(header + pcm)
        process.stdin.flush()
        if process.stdout.read(4) != header:
            raise BenchFault("GNU Radio returned an invalid PCM header")
        result = process.stdout.read(len(pcm))
        if len(result) != len(pcm):
            raise BenchFault("GNU Radio returned a truncated PCM record")
        return result

    def stock_bursts(self, source, role):
        history = deque(maxlen=5)
        active = []
        quiet = 0
        buffered = bytearray()
        index = 0
        for pcm in self.record_stream(source, role):
            buffered.extend(pcm)
            while len(buffered) >= PCM_WINDOW * 4:
                window = bytes(buffered[:PCM_WINDOW * 4])
                del buffered[:PCM_WINDOW * 4]
                samples = array.array("f", window)
                loud = sum(sample * sample for sample in samples) >= PCM_WINDOW * 0.0001
                if loud and not active:
                    active.extend(history)
                if active:
                    active.append(window)
                    quiet = 0 if loud else quiet + 1
                    if quiet >= 10 or len(active) >= 590:
                        burst = b"".join(active)
                        self.save_burst(role, index, burst)
                        yield index, burst
                        index += 1
                        active.clear()
                        quiet = 0
                history.append(window)

    def stock_to_linkwise(self, source, target, role):
        for index, burst in self.stock_bursts(source, role):
            if not self.drop_stock_ack(role, index, burst):
                pcm = self.stock_data_fault(role, index, burst)
                write_pcm(target, self.impair(pcm, role))

    def stock_data_fault(self, role, index, burst):
        if role != 0 or self.phase != "first delivery":
            return burst
        frames = len(burst) // 4
        if self.fault_injected:
            if self.args.raise_stock_rate and 4.8 * SAMPLE_RATE <= frames <= 5.4 * SAMPLE_RATE:
                self.stock_retry_seen = True
            return burst
        if self.received[1] or not 4 * SAMPLE_RATE <= frames <= 5 * SAMPLE_RATE:
            return burst
        if self.args.corrupt_stock_first:
            label = "stock first DATA"
        elif self.args.corrupt_stock_after_prefix and b"BUFFER 157\r" in self.commands[0]:
            label = "stock DATA after acknowledged prefix"
        else:
            return burst
        self.save_fault(self.args.replacement, index, label)
        return self.args.replacement

    def drop_stock_ack(self, role, index, burst):
        if (
            not self.args.drop_stock_ack or self.fault_injected or role != 1
            or self.phase != "first delivery"
        ):
            return False
        delivered = len(self.received[1])
        frames = len(burst) // 4
        if self.args.drop_stock_ack == "intermediate":
            match = delivered == 43 and 20_000 <= frames <= 27_000
        else:
            match = delivered == self.args.payload_length and 27_000 <= frames <= 35_000
        if match:
            (self.args.output / "dropped.f32").write_bytes(burst)
            self.fault_injected = True
            self.log("FAULT withheld stock", self.args.drop_stock_ack, "acknowledgment", index)
        return match

    def play_to_stock(self, source, target, role, stock_source=False):
        frames = queue.Queue()

        def receive():
            if stock_source:
                for _, pcm in self.stock_bursts(source, role):
                    frames.put(self.impair(pcm, role))
                return
            for index, pcm in enumerate(self.record_stream(source, role)):
                self.save_burst(role, index, pcm)
                if self.drop_linkwise_ack(role, index, pcm):
                    continue
                if self.args.expected_sha and not self.fault_injected and sha256(pcm) == self.args.expected_sha:
                    pcm = self.args.replacement
                    self.save_fault(pcm, index, "guarded Linkwise burst")
                if (
                    self.args.raise_stock_rate and self.stock_retry_seen
                    and not self.rate_reply_injected and sha256(pcm) == self.args.rate_ack_sha
                ):
                    pcm = self.args.rate_reply
                    (self.args.output / "injected-rate-reply.f32").write_bytes(pcm)
                    self.rate_reply_injected = True
                    self.log("FAULT substituted captured rate-increase acknowledgment", index)
                frames.put(self.impair(pcm, role))

        self.worker(receive)
        tick = time.monotonic()
        pending = bytearray()
        silence = bytes(PCM_TICK * 4)
        name = ("caller", "responder")[role]
        with (self.args.output / f"{name}-playback.f32").open("wb") as audio, (
            self.args.output / f"{name}-playback.tsv"
        ).open("w") as timing:
            while True:
                time.sleep(max(0, tick - time.monotonic()))
                tick += PCM_TICK / SAMPLE_RATE
                if not pending:
                    try:
                        pending.extend(frames.get_nowait())
                    except queue.Empty:
                        pass
                chunk = bytes(pending[:len(silence)]).ljust(len(silence), b"\x00")
                del pending[:len(silence)]
                write_pcm(target, chunk)
                audio.write(chunk)
                print(f"{time.monotonic() - self.start:.6f}\t{PCM_TICK}", file=timing)

    def drop_linkwise_ack(self, role, index, pcm):
        kind = self.args.drop_linkwise_ack
        if not kind or self.fault_injected or role != 1 or self.phase != "first delivery":
            return False
        if kind == "intermediate":
            match = len(pcm) == 16416 * 4
        else:
            buffers = re.findall(rb"BUFFER (\d+)\r", self.commands[0])
            match = len(pcm) == 22560 * 4 and buffers and 0 < int(buffers[-1]) <= 43
        if match:
            (self.args.output / "dropped.f32").write_bytes(pcm)
            self.fault_injected = True
            self.log("FAULT withheld Linkwise", kind, "acknowledgment", index)
        return match

    def save_fault(self, pcm, index, label):
        (self.args.output / "injected.f32").write_bytes(pcm)
        self.fault_injected = True
        self.log("FAULT replaced", label, index)

    def relay_stock(self, source, target, role):
        for pcm in self.record_stream(source, role):
            if self.args.pace_virtual:
                time.sleep(len(pcm) / (4 * SAMPLE_RATE))
            write_pcm(target, self.impair(pcm, role))

    def worker(self, target, *args):
        def run():
            try:
                target(*args)
            except (BenchFault, EOFError, OSError) as error:
                with self.changed:
                    if self.tearing_down or self.transport_fault is not None:
                        return
                    detail = str(error) or type(error).__name__
                    self.transport_fault = BenchFault(f"audio transport: {detail}")
                    self.closed.add("audio")
                    self.changed.notify_all()
                self.log("AUDIO FAULT", self.transport_fault)

        thread = threading.Thread(target=run, daemon=True)
        thread.start()
        return thread

    def receive_host(self, stream, role, command):
        channel = "command" if command else "data"
        try:
            while chunk := stream.recv(4096):
                with self.changed:
                    destination = self.commands if command else self.received
                    destination[role].extend(chunk)
                    self.log(role, channel, repr(chunk) if command else len(chunk))
                    self.changed.notify_all()
        finally:
            with self.changed:
                self.closed.add((role, channel))
                self.changed.notify_all()

    def wait_for(self, condition, seconds):
        with self.changed:
            self.changed.wait_for(lambda: condition() or self.closed, timeout=seconds)
            if self.transport_fault is not None:
                raise self.transport_fault
            if not condition():
                raise FixtureFailure(f"{self.phase}: timeout or closed channel {sorted(self.closed, key=str)}")

    def command(self, stream, text):
        self.log("send command", text)
        stream.sendall(text.encode("ascii") + b"\r")

    def run(self):
        stock_roles = {
            "stock": (0, 1), "linkwise-caller": (1,), "linkwise-responder": (0,),
            "linkwise": (),
        }[self.args.case]
        with ExitStack() as stack:
            if self.args.hf_snr:
                delay, spread, echo, seed = HF_PROFILES[self.args.hf_snr]
                for role in range(2):
                    stderr = stack.enter_context((self.args.output / f"gnuradio-{role}.stderr").open("w"))
                    process = subprocess.Popen(
                        ["docker", "run", "--rm", "-i", "-e", "BENCH_RICIAN_K=10",
                         "-v", f"{self.args.hf_worker.resolve()}:/bench-channel.py:ro",
                         "linkwise-hf-bench", "python3", "-u", "/bench-channel.py",
                         str(self.args.hf_snr), str(delay), str(spread), str(echo), str(seed)],
                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr,
                    )
                    stack.callback(process.stdout.close)
                    stack.callback(process.stdin.close)
                    stack.callback(stop, process)
                    self.hf_workers[role] = process
            channels = [None, None]
            listeners = {}
            for role in stock_roles:
                listeners[role] = []
                base = self.args.stock_bridge_base or BASE_PORTS[role]
                for port in range(base, base + 3):
                    listener = stack.enter_context(socket.create_server(("127.0.0.1", port)))
                    listener.settimeout(45)
                    listeners[role].append(listener)
            self.log(
                "listeners ready",
                ", ".join(str(self.args.stock_bridge_base or BASE_PORTS[role]) for role in stock_roles),
            )
            for role in stock_roles:
                channels[role] = [stack.enter_context(listener.accept()[0]) for listener in listeners[role]]
            for role in {0, 1} - set(stock_roles):
                channels[role] = self.start_linkwise(stack, role)
            for role in stock_roles:
                channels[role][2].sendall(b"\x01")

            workers = []
            for role, (command, data, audio) in enumerate(channels):
                target = channels[1 - role][2]
                if len(stock_roles) == 2 and self.args.hf_snr:
                    workers.append(self.worker(self.play_to_stock, audio, target, role, True))
                elif len(stock_roles) in (0, 2):
                    workers.append(self.worker(self.relay_stock, audio, target, role))
                elif role in stock_roles:
                    workers.append(self.worker(self.stock_to_linkwise, audio, target, role))
                else:
                    workers.append(self.worker(self.play_to_stock, audio, target, role))
                workers.append(self.worker(self.receive_host, command, role, True))
                workers.append(self.worker(self.receive_host, data, role, False))
            try:
                self.exchange(channels)
                with self.changed:
                    if self.transport_fault is not None:
                        raise self.transport_fault
                    self.tearing_down = True
            finally:
                with self.changed:
                    self.tearing_down = True
                for sockets in channels:
                    for stream in sockets:
                        try:
                            stream.shutdown(socket.SHUT_RDWR)
                        except OSError:
                            pass
                for worker in workers:
                    worker.join(timeout=2)

    def exchange(self, channels):
        self.phase = "setup"
        for (command, _, _), callsign in zip(channels, CALLSIGNS):
            self.command(command, "ABORT")
            time.sleep(2)
            for text in (
                f"MYCALL {callsign}", f"BW{self.args.bandwidth}", "P2P SESSION",
                "COMPRESSION OFF", "CWID OFF", "VERSION",
            ):
                self.command(command, text)
        caller_command, caller_data, _ = channels[0]
        peer_command, peer_data, _ = channels[1]
        self.command(peer_command, "LISTEN ON")
        time.sleep(2)
        self.phase = "connect"
        connect_started = time.monotonic()
        self.command(caller_command, "CONNECT KN6MQS W1A")
        connected = f"CONNECTED KN6MQS W1A {self.args.bandwidth}\r".encode("ascii")
        self.wait_for(lambda: all(connected in messages for messages in self.commands), 35)
        self.measurements["connect_seconds"] = time.monotonic() - connect_started
        first = bytes(range(self.args.payload_length))
        second = first[::-1]
        reply = RETURN_PAYLOAD if self.args.reply_length is None else bytes(range(self.args.reply_length))[::-1]
        deliveries = (
            ("first delivery", caller_data, first, 1, first),
            ("reverse delivery", peer_data, reply, 0, reply),
            ("second delivery", caller_data, second, 1, first + second),
        )
        expected_streams = [reply, first + second]
        late_delay = self.args.late_caller_write_delay
        if late_delay is not None:
            deliveries = (
                deliveries[0],
                ("late caller write", caller_data, b"\x01\x00", 1, first + b"\x01\x00"),
            )
            expected_streams = [b"", first + b"\x01\x00"]
        self.expected_streams = expected_streams
        transfer_started = time.monotonic()
        for phase, stream, payload, role, expected in deliveries:
            self.phase = phase
            self.log(phase, "queue", len(payload))
            caller_offset = len(self.commands[0])
            delivery_started = time.monotonic()
            stream.sendall(payload)
            self.wait_for(lambda: len(self.received[role]) >= len(expected), 85)
            if self.received[role] != expected:
                raise FixtureFailure(f"{phase}: payload mismatch")
            self.measurements[phase + "_seconds"] = time.monotonic() - delivery_started
            self.log(phase, "exact", len(payload))
            if phase == "first delivery" and (self.args.drop_stock_ack or self.args.drop_linkwise_ack == "final"):
                self.wait_for(lambda: b"BUFFER 0\r" in self.commands[0][caller_offset:], 35)
            if phase == "first delivery" and late_delay is not None:
                if late_delay >= 0:
                    self.wait_for(lambda: b"BUFFER 0\r" in self.commands[0][caller_offset:], 15)
                    time.sleep(late_delay)
                elif b"BUFFER 0\r" in self.commands[0][caller_offset:]:
                    raise BenchFault("the caller queue drained before the late write")
            if phase == deliveries[-1][0] and self.args.verify_disconnect:
                self.wait_for(lambda: b"BUFFER 0\r" in self.commands[0][caller_offset:], 15)
        if (self.args.replacement or self.args.drop_stock_ack or self.args.drop_linkwise_ack) and not self.fault_injected:
            raise BenchFault("guarded fault stimulus was never sent")
        if self.args.raise_stock_rate and not self.rate_reply_injected:
            raise BenchFault("guarded rate acknowledgment was never sent")
        self.measurements["transfer_seconds"] = time.monotonic() - transfer_started
        self.phase = "disconnect"
        disconnect_started = time.monotonic()
        command_offsets = [len(messages) for messages in self.commands]
        closing_command = peer_command if self.args.disconnect_from == "responder" else caller_command
        self.command(closing_command, "DISCONNECT")
        def observed_disconnect():
            self.disconnected = [
                b"DISCONNECTED\r" in messages[offset:]
                for messages, offset in zip(self.commands, command_offsets)
            ]
            return all(self.disconnected)

        if self.args.verify_disconnect:
            self.wait_for(observed_disconnect, 15)
            self.measurements["disconnect_seconds"] = time.monotonic() - disconnect_started
        else:
            time.sleep(3)
            observed_disconnect()
        if self.received != expected_streams:
            raise FixtureFailure("duplicate or unexpected host DATA")
        if self.args.verify_reconnect:
            self.phase = "reconnect"
            command_offsets = [len(messages) for messages in self.commands]
            self.command(caller_command, "CONNECT KN6MQS W1A")
            self.wait_for(
                lambda: all(
                    connected in messages[offset:]
                    for messages, offset in zip(self.commands, command_offsets)
                ),
                35,
            )
            probe = b"fresh link\x00\xff"
            caller_data.sendall(probe)
            self.wait_for(lambda: len(self.received[1]) >= len(expected_streams[1] + probe), 30)
            if self.received != [expected_streams[0], expected_streams[1] + probe]:
                raise FixtureFailure("reconnect: duplicate or unexpected host DATA")
            self.wait_for(lambda: b"BUFFER 0\r" in self.commands[0][command_offsets[0]:], 15)
            command_offsets = [len(messages) for messages in self.commands]
            self.command(closing_command, "DISCONNECT")
            self.wait_for(observed_disconnect, 15)
            self.log("reconnect exact", len(probe))
        sizes = " + ".join(str(len(delivery[2])) for delivery in deliveries)
        self.log("PASS", f"{sizes} exact bytes")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("case", choices=("stock", "linkwise", "linkwise-caller", "linkwise-responder"))
    parser.add_argument("output", type=Path, help="new directory for the transcript and PCM")
    parser.add_argument("--modem", type=Path)
    parser.add_argument("--profile", type=Path)
    parser.add_argument("--bandwidth", type=int, default=500, choices=(500, 2300, 2750))
    parser.add_argument("--payload-length", type=int)
    parser.add_argument("--reply-length", type=int, help="send a binary reply of this length")
    parser.add_argument("--stock-bridge-base", type=int, help="override the bridge base port for a mixed run")
    parser.add_argument("--verify-disconnect", action="store_true", help="require both endpoints to report disconnect after DATA drains")
    parser.add_argument("--disconnect-from", choices=("caller", "responder"), default="caller")
    parser.add_argument("--verify-reconnect", action="store_true", help="require a fresh exact transfer on the same host sockets")
    parser.add_argument("--late-caller-write-delay", type=float, help="send a two-byte caller continuation after BUFFER 0, or before the final ACK with -1")
    impairment = parser.add_mutually_exclusive_group()
    impairment.add_argument("--replace-burst", nargs=2, type=Path, metavar=("ORIGINAL_F32", "REPLACEMENT_WAV"))
    impairment.add_argument("--corrupt-stock-first", type=Path, metavar="REPLACEMENT_WAV")
    impairment.add_argument("--corrupt-stock-after-prefix", type=Path, metavar="REPLACEMENT_WAV")
    impairment.add_argument("--drop-stock-ack", choices=("intermediate", "final"))
    impairment.add_argument("--drop-linkwise-ack", choices=("intermediate", "final"))
    parser.add_argument("--raise-stock-rate", nargs=2, type=Path, metavar=("ACK_F32", "INCREASE_F32"))
    parser.add_argument("--pace-virtual", action="store_true",
                        help="wait each burst's audio duration in the two-Linkwise case")
    parser.add_argument("--hf-snr", type=int, choices=HF_PROFILES)
    parser.add_argument("--hf-worker", type=Path, help="GNU Radio framed-PCM worker script")
    args = parser.parse_args()
    if args.hf_snr and (args.bandwidth != 500 or not args.hf_worker or not args.hf_worker.is_file()):
        parser.error("--hf-snr requires BW500 and an existing --hf-worker script")
    if args.pace_virtual and args.case != "linkwise":
        parser.error("--pace-virtual requires the two-Linkwise case")
    if args.late_caller_write_delay is not None and (
        args.bandwidth != 500
        or not (args.late_caller_write_delay == -1 or 0 <= args.late_caller_write_delay <= 15)
    ):
        parser.error("late caller writes require BW500 and a delay of -1 or 0–15 seconds")
    if args.verify_reconnect:
        args.verify_disconnect = True
    args.payload_length = args.payload_length if args.payload_length is not None else (203 if args.bandwidth == 500 else 13)
    if not 1 <= args.payload_length <= 256:
        parser.error("payload length must be between 1 and 256 bytes")
    if args.reply_length is not None and not 1 <= args.reply_length <= 256:
        parser.error("reply length must be between 1 and 256 bytes")
    if args.stock_bridge_base is not None and (
        args.case in ("stock", "linkwise") or not 1024 <= args.stock_bridge_base <= 65533
    ):
        parser.error("--stock-bridge-base requires one stock endpoint and three valid ports")
    if args.case != "stock" and (args.modem is None or (args.bandwidth == 500 and args.profile is None)):
        parser.error("Linkwise runs require --modem and BW500 also requires --profile")
    if args.case == "stock" and args.replace_burst:
        parser.error("burst replacement requires a Linkwise endpoint")
    if args.corrupt_stock_first and (args.case != "linkwise-responder" or args.bandwidth != 500):
        parser.error("stock first-frame corruption requires the BW500 Linkwise-responder case")
    if args.corrupt_stock_after_prefix and (
        args.case != "linkwise-responder" or args.bandwidth != 500 or args.payload_length != 200
    ):
        parser.error("stock prefix corruption requires the 200-byte BW500 Linkwise-responder case")
    if args.drop_stock_ack and (args.case != "linkwise-caller" or args.bandwidth != 500 or args.payload_length != 200):
        parser.error("stock acknowledgment loss requires the 200-byte BW500 Linkwise-caller case")
    if args.drop_linkwise_ack and (args.case != "linkwise-responder" or args.bandwidth != 500 or args.payload_length != 200):
        parser.error("Linkwise acknowledgment loss requires the 200-byte BW500 Linkwise-responder case")
    if args.raise_stock_rate:
        if not args.corrupt_stock_first:
            parser.error("--raise-stock-rate requires --corrupt-stock-first")
        try:
            expected_ack, increase = args.raise_stock_rate
            args.rate_ack_sha = file_sha256(expected_ack)
            args.rate_reply = increase.read_bytes()
            if len(args.rate_reply) % 4 or not 0 < len(args.rate_reply) <= MAX_RECORD * 4:
                raise ValueError("rate reply must contain one valid f32 audio record")
        except (OSError, ValueError) as error:
            parser.error(str(error))
    args.replacement = None
    args.expected_sha = None
    if args.replace_burst or args.corrupt_stock_first or args.corrupt_stock_after_prefix:
        try:
            if args.replace_burst:
                original, replacement = args.replace_burst
                args.replacement = read_replacement(replacement)
                original_pcm = original.read_bytes()
                if len(args.replacement) != len(original_pcm):
                    raise ValueError("the replacement must have the same length as the guarded burst")
                args.expected_sha = sha256(original_pcm)
            else:
                args.replacement = read_replacement(args.corrupt_stock_first or args.corrupt_stock_after_prefix)
            if not 0 < len(args.replacement) <= MAX_RECORD * 4:
                raise ValueError("the replacement must contain one valid audio record")
        except (OSError, ValueError, wave.Error) as error:
            parser.error(str(error))

    args.output.mkdir(parents=True)
    manifest = {
        "case": args.case,
        "hf_snr_db": args.hf_snr,
        "hf_delay_spread_echo_seed": HF_PROFILES.get(args.hf_snr),
        "hf_worker_sha256": file_sha256(args.hf_worker) if args.hf_worker else None,
        "pace_virtual": args.pace_virtual,
        "bandwidth": args.bandwidth,
        "payload_length": args.payload_length,
        "reply_length": args.reply_length if args.reply_length is not None else len(RETURN_PAYLOAD),
        "stock_bridge_base": args.stock_bridge_base,
        "verify_disconnect": args.verify_disconnect,
        "disconnect_from": args.disconnect_from,
        "verify_reconnect": args.verify_reconnect,
        "late_caller_write_delay": args.late_caller_write_delay,
        "modem_sha256": file_sha256(args.modem) if args.modem else None,
        "profile_sha256": file_sha256(args.profile) if args.profile else None,
        "expected_burst_sha256": args.expected_sha,
        "corrupt_stock_first": bool(args.corrupt_stock_first),
        "corrupt_stock_after_prefix": bool(args.corrupt_stock_after_prefix),
        "drop_stock_ack": args.drop_stock_ack,
        "drop_linkwise_ack": args.drop_linkwise_ack,
        "rate_reply_pcm_sha256": sha256(args.rate_reply) if args.raise_stock_rate else None,
        "replacement_pcm_sha256": sha256(args.replacement) if args.replacement else None,
    }
    with (args.output / "transcript.log").open("w") as transcript:
        trial = Trial(args, transcript)
        try:
            trial.run()
            manifest["result"] = "pass"
        except (BenchFault, FixtureFailure, OSError) as error:
            missing_fault = (args.replacement or args.drop_stock_ack or args.drop_linkwise_ack) and not trial.fault_injected
            missing_rate_reply = args.raise_stock_rate and not trial.rate_reply_injected
            if isinstance(error, FixtureFailure) and not (missing_fault or missing_rate_reply):
                manifest["result"] = "fail"
            else:
                manifest["result"] = "bench fault"
            manifest["reason"] = str(error)
            trial.log(manifest["result"].upper(), error)
        manifest["phase"] = trial.phase
        manifest["measurements"] = trial.measurements
        manifest["expected_sha256"] = [sha256(data) for data in trial.expected_streams] if trial.expected_streams else None
        manifest["received_lengths"] = [len(data) for data in trial.received]
        manifest["received_sha256"] = [sha256(data) for data in trial.received]
        manifest["recorded_audio_seconds"] = {
            role: path.stat().st_size / (4 * SAMPLE_RATE)
            for role in ("caller", "responder")
            if (path := args.output / f"{role}-stream.f32").exists()
        }
        manifest["fault_injected"] = trial.fault_injected
        manifest["rate_reply_injected"] = trial.rate_reply_injected
        manifest["disconnect_confirmed"] = trial.disconnected
        for role, data in enumerate(trial.received):
            (args.output / f"received-{role}.bin").write_bytes(data)
        (args.output / "result.json").write_text(json.dumps(manifest, indent=2) + "\n")
    return 0 if manifest["result"] == "pass" else 1


if __name__ == "__main__":
    raise SystemExit(main())
