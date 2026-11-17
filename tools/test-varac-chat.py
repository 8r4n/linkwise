#!/usr/bin/env python3
"""Exercise both chat terminals over the Linkwise BW500 waveform bench (Unix PTY)."""
import argparse
from contextlib import ExitStack
import fcntl
import importlib.util
from pathlib import Path
import pty
import os
import signal
import socket
import struct
import subprocess
import termios
import threading

spec = importlib.util.spec_from_file_location(
    "waveform_bench", Path(__file__).with_name("test-virtual-bw500.py")
)
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)


class Chat:
    def __init__(self, stack, cli, callsign, ports, options=()):
        self.pty, slave = pty.openpty()
        stack.callback(os.close, self.pty)
        stack.callback(os.close, slave)
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 85, 0, 0))
        self.baseline = termios.tcgetattr(slave)
        self.slave = slave
        self.process = subprocess.Popen(
            [cli, "chat", *options, callsign, *map(str, ports[:2])],
            stdin=slave,
            stdout=slave,
            stderr=slave,
            # Crossterm otherwise reads the launching terminal's size from /dev/tty.
            start_new_session=True,
        )
        stack.callback(self.stop)
        self.output = bytearray()
        self.changed = threading.Condition()
        self.eof = False
        threading.Thread(target=self.read, daemon=True).start()
        self.expect("Attached to Linkwise modem")

    def stop(self):
        if self.process.poll() is None:
            self.process.kill()
        self.process.wait()

    def enter(self, text):
        os.write(self.pty, text.encode() + b"\r")

    def resize(self, columns, rows=24):
        with self.changed:
            self.output.clear()
            fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))
            self.process.send_signal(signal.SIGWINCH)

    def read(self):
        try:
            while data := os.read(self.pty, 65536):
                with self.changed:
                    self.output.extend(data)
                    self.changed.notify_all()
        except OSError:
            pass
        finally:
            with self.changed:
                self.eof = True
                self.changed.notify_all()

    def expect(self, text, seconds=45):
        needle = text.encode()
        with self.changed:
            self.changed.wait_for(lambda: self.eof or needle in self.output, seconds)
            index = self.output.find(needle)
            if index < 0:
                raise AssertionError(f"Missing {text!r} in terminal tail {self.output[-2000:]!r}")
            del self.output[:index + len(needle)]

    def quit(self):
        self.enter("/quit")
        assert self.process.wait(timeout=10) == 0
        assert termios.tcgetattr(self.slave) == self.baseline


def relay(source, destination):
    try:
        while True:
            destination.sendall(bench.waveform(source))
    except (OSError, EOFError):
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cli")
    parser.add_argument("modem")
    parser.add_argument("profile")
    args = parser.parse_args()
    local, peer = "KN6MQS-15", "BB2BB"
    with ExitStack() as stack:
        ports = [bench.start_station(stack, args.modem, args.profile, call, 500)
                 for call in (local, peer)]
        audio = [stack.enter_context(socket.create_connection(("127.0.0.1", p[2]))) for p in ports]
        for source, target in (audio, audio[::-1]):
            threading.Thread(target=relay, args=(source, target), daemon=True).start()

        a = Chat(stack, args.cli, local, ports[0])
        b = Chat(stack, args.cli, peer, ports[1])
        b.enter("/listen on")
        b.expect("Listening enabled")
        a.enter(f"/connect {peer}")
        a.expect("Connected at BW500")
        b.expect("Connected at BW500")
        a.expect(f"{peer} joined the chat")
        b.expect(f"{local} joined the chat")
        a.enter("/who")
        a.expect(f"You: {local}")
        a.expect(f"Peer: {peer} (connected)")

        a.enter("Linkwise first message")
        b.expect(f"<{local:9}   > Linkwise first message")
        a.expect("Modem acknowledged queued text")
        b.enter("A longer reply " + "radio " * 40 + "END-OF-REPLY")
        a.expect(f"<{peer:9}   > A longer reply")
        a.expect("END-OF-REPLY")
        b.expect("Modem acknowledged queued text")
        for columns, first_row in [
            (120, "A longer reply" + " radio" * 15),
            (40, "A longer reply radio"),
            (24, "A longer"),
            (16, ""),
        ]:
            a.resize(columns, rows=42)
            prefix = f"<{peer:9}   >"
            expected = f"{prefix} {first_row}" if first_row else prefix
            a.expect(expected + "\x1b[")
            a.expect("END-OF-REPLY")
        a.resize(85)
        a.enter("/disconnect")
        a.expect(f"{peer} left the chat")
        b.expect(f"{local} disconnected")
        a.enter("/who")
        a.expect("No peer connected.")

        a.enter("/listen on")
        a.expect(" | listen on | ")
        b.enter(f"/connect {local}")
        b.expect("Connected at BW500")
        a.expect("Connected at BW500")
        a.expect(f"{peer} joined the chat")
        b.expect(f"{local} joined the chat")
        b.enter("Text after reconnect")
        b.expect(f"<{peer:9}   > Text after reconnect")
        b.enter("/disconnect")
        a.expect(f"<{peer:9}   > Text after reconnect")
        b.expect(f"{local} left the chat")
        a.expect(f"{peer} disconnected")
        for terminal in (a, b):
            terminal.quit()
    print("BW500 chat passed in both call directions with reconnect and terminal restoration.")


if __name__ == "__main__":
    main()
