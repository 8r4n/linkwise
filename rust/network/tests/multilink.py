"""Pass both chat clients' outbound DATA through three durable BP hops before BW500."""
import contextlib
import socket
import subprocess
import threading


def attach(stack, network, root, name, ports):
    listener = stack.enter_context(socket.socket())
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    counts = []
    errors = []
    chain = ("A", "D", "E", "F") if name == "left" else ("F", "E", "D", "A")
    directory = root / name
    directory.mkdir()

    def transfer(data):
        payload = directory / "input"
        payload.write_bytes(data)
        def run(command, node, *args):
            return subprocess.check_output([network, command, directory / node, node, *args],
                                           timeout=15)
        ident = run("submit", chain[0], chain[-1], payload, "300").decode().strip()
        for source, destination in zip(chain, chain[1:]):
            with subprocess.Popen([network, "listen", directory / destination, destination,
                                   "127.0.0.1:0"], stdout=subprocess.PIPE) as receiver:
                try:
                    address = receiver.stdout.readline().decode().strip()
                    run("send", source, ident, address)
                    output, _ = receiver.communicate(timeout=10)
                    assert receiver.returncode == 0 and output.decode().strip() == ident
                finally:
                    if receiver.poll() is None:
                        receiver.kill()
        result = run("read", chain[-1], ident)
        assert result == data
        counts.append(len(data))
        return result

    def forward():
        try:
            client, _ = listener.accept()
            with client, socket.create_connection(("127.0.0.1", ports[1])) as modem:
                def inbound():
                    try:
                        while data := modem.recv(32768):
                            client.sendall(data)
                    except OSError:
                        pass
                threading.Thread(target=inbound, daemon=True).start()
                while data := client.recv(32768):
                    modem.sendall(transfer(data))
        except Exception as error:
            errors.append(error)

    worker = threading.Thread(target=forward, daemon=True)
    worker.start()

    def verify():
        worker.join(timeout=20)
        if errors:
            raise errors[0]
        assert not worker.is_alive() and counts
        print(f"{name} {','.join(chain)}: {len(counts)} bundles, {sum(counts)} bytes")
    stack.callback(verify)
    return [ports[0], listener.getsockname()[1], ports[2]]
