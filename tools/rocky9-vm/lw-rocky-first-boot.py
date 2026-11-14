import socket,sys,time
from pathlib import Path
folder=Path(sys.argv[1])
for _ in range(150):
    try:
        stream=socket.create_connection(('127.0.0.1',2234),timeout=1)
        break
    except OSError: time.sleep(.1)
else: raise RuntimeError('serial endpoint unavailable')
with stream,(folder/(sys.argv[2] if len(sys.argv) > 2 else 'first-boot.log')).open('ab',buffering=0) as log:
    stream.settimeout(4)
    pending=b''
    unlock_count=0
    probe = len(sys.argv) > 3 and sys.argv[3] == "check-grub"
    edit_attempted = False
    authentication_required = False
    denied = False
    returned_to_menu = False
    end=time.monotonic()+240
    while time.monotonic()<end:
        try: data=stream.recv(8192)
        except socket.timeout: continue
        if not data: raise ConnectionError("serial endpoint closed before guest login")
        log.write(data)
        pending=(pending+data)[-4096:]
        if probe and not edit_attempted and b'Use the ^ and v keys' in pending:
            stream.sendall(b'e')
            edit_attempted = True
            pending = b''
        if probe and edit_attempted and not authentication_required and b'Enter username' in pending:
            authentication_required = True
            stream.sendall(b'\x1b')
            pending = b''
        if probe and authentication_required and not denied and b'Press any key to continue' in pending:
            denied = b'access denied' in pending
            stream.sendall(b' ')
            pending = b''
        if probe and denied and not returned_to_menu and b'Use the ^ and v keys' in pending:
            stream.sendall(b'\r')
            returned_to_menu = True
            pending = b''
        if b'passphrase' in pending.lower() and (b'please' in pending.lower() or b'unlock' in pending.lower()):
            if unlock_count>=2: raise RuntimeError('disk unlock was requested repeatedly')
            stream.sendall((folder/'disk-unlock.txt').read_bytes().strip()+b'\r')
            pending=b''
            unlock_count+=1
        if b'login:' in pending:
            if probe and not denied:
                raise RuntimeError('GRUB edit authentication prompt was not observed')
            print(f'Reached login prompt after {unlock_count} unlock attempts')
            break
    else: raise TimeoutError('no guest login prompt')
