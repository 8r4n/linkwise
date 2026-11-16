#!/usr/bin/env python3
"""Exercise file commands in two chat terminals over virtual BW500 audio."""

import argparse
from contextlib import ExitStack
import hashlib
import json
from pathlib import Path
import runpy
import socket
import tempfile
import threading

SOURCE = Path(__file__)
CHAT = runpy.run_path(str(SOURCE.with_name("test-varac-chat.py")))
SECURE = runpy.run_path(str(SOURCE.with_name("test-secure-transfer.py")))


def file_pair(args, scenario):
    print(f"{scenario}: starting virtual BW500 bench", flush=True)
    calls = SECURE["CALLS"]
    with ExitStack() as stack:
        directory = Path(stack.enter_context(tempfile.TemporaryDirectory()))
        audio, terminals, options = [], [], [(), ()]
        ports = [CHAT["bench"].start_station(stack, args.modem, args.profile, call, 500)
                 for call in calls]
        for port in ports:
            audio.append(stack.enter_context(socket.create_connection(("127.0.0.1", port[2]))))
        for source, destination in (audio, audio[::-1]):
            threading.Thread(target=CHAT["relay"], args=(source, destination), daemon=True).start()

        if scenario in ("secure", "pin", "unexpected-peer"):
            SECURE["certificates"](directory)
            for index, call in enumerate(calls):
                peer = "OTHER" if scenario == "unexpected-peer" and index == 1 else calls[1 - index]
                pin = "OTHER" if scenario == "pin" and index == 0 else peer
                settings = dict(
                    local_call=call, peer_call=peer, peer_dns=peer.lower() + ".test",
                    certificate=str(directory / f"{call}.pem"),
                    private_key=str(directory / f"{call}.key"),
                    peer_certificate=str(directory / f"{pin}.pem"),
                    ca_certificate=str(directory / "ca.pem"),
                    command_port=ports[index][0], data_port=ports[index][1], max_bytes=4096,
                )
                config = directory / f"{call}.json"
                config.write_text(json.dumps(dict(encryption=dict(enabled=True, settings=settings))))
                options[index] = ["--secure-config", str(config)]

        for call, port, option in zip(calls, ports, options):
            terminals.append(CHAT["Chat"](stack, args.cli, call, port, option))
        sender, receiver = terminals
        original = directory / "send sample.bin"
        destination = directory / "received.bin"
        # Incompressible input makes the plain body span multiple host DATA writes.
        blocks = 17 if scenario in ("secure", "pin") else 70
        payload = b"".join(hashlib.sha256(bytes([i])).digest() for i in range(blocks))
        original.write_bytes(payload)

        if scenario == "unexpected-peer":
            receiver.enter(f"/receivefile --encrypt {destination}")
            receiver.expect("Linkwise encrypted file transfer in progress")
            sender.enter(f"/connect {calls[1]}")
            sender.expect(f"Calling {calls[1]}")
            receiver.expect("Encrypted file transfer failed: unexpected modem peer")
            receiver.expect("Attached to Linkwise modem")
            assert not destination.exists()
            sender.enter("/disconnect")
            sender.expect(" | idle | ")
            receiver.enter("/listen on")
            receiver.expect("Listening enabled")
            sender.enter(f"/connect {calls[1]}")
            for terminal in terminals:
                terminal.expect("Connected at BW500")
            sender.enter("Chat after failed handoff")
            receiver.expect(f"<{calls[0]:9}   > Chat after failed handoff")
            sender.enter("/disconnect")
            for terminal in terminals:
                terminal.expect(" | idle | ")
        elif scenario in ("secure", "pin"):
            receiver.enter(f"/receivefile --encrypt {destination}")
            receiver.expect("Linkwise encrypted file transfer in progress")
            sender.enter(f"/sendfile --encrypt {original}")
            sender.expect("Linkwise encrypted file transfer in progress")
            expected = ("Encrypted file transfer complete" if scenario == "secure"
                        else "Encrypted file transfer failed")
            sender.expect(expected, seconds=120)
            receiver.expect(expected, seconds=120)
            if scenario == "pin":
                assert not destination.exists()
        else:
            receiver.enter("/listen on")
            receiver.expect("Listening enabled")
            sender.enter(f"/connect {calls[1]}")
            for terminal in terminals:
                terminal.expect("Connected at BW500")
            sender.enter(f"/sendfile {original}")
            receiver.expect("File 1 offered")
            receiver.enter("/reject 1")
            sender.expect("Peer rejected or aborted file 1")
            assert not destination.exists()
            retry = directory / "accepted sample.bin"
            retry.write_bytes(payload)
            sender.enter(f"/sendfile {retry}")
            receiver.expect("File 1 offered: accepted sample.bin")
            receiver.enter(f"/accept 1 {destination}")
            receiver.expect("Received file 1", seconds=120)
            sender.expect("File 1 confirmed by peer", seconds=120)
            sender.enter("Chat after file transfer")
            receiver.expect(f"<{calls[0]:9}   > Chat after file transfer")
            sender.expect("Modem acknowledged queued text")
            if scenario == "plain":
                cancelled = directory / "cancelled.bin"
                cancelled.write_bytes(payload)
                sender.enter(f"/sendfile {cancelled}")
                receiver.expect("File 1 offered: cancelled.bin")
                sender.enter("/cancel 1")
                sender.expect("File 1 cancelled")
                receiver.expect("Peer rejected or aborted file 1")

        if scenario in ("plain", "secure"):
            assert destination.read_bytes() == payload
        for terminal in terminals:
            terminal.quit()
        print(f"{scenario}: passed", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cli")
    parser.add_argument("modem")
    parser.add_argument("profile")
    parser.add_argument("scenarios", choices=("plain", "secure", "pin", "unexpected-peer"), nargs="+")
    args = parser.parse_args()
    for scenario in args.scenarios:
        file_pair(args, scenario)
