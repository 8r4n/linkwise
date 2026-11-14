import socket,sys,time
from pathlib import Path
p=Path(sys.argv[1])
boot_arg = (" boot=UUID=" + str(__import__("uuid").UUID(sys.argv[2])) + " audit_backlog_limit=8192").encode()
with socket.create_connection(('127.0.0.1',2234),timeout=1) as s, (p/'fips-recovery-boot.log').open('wb',buffering=0) as log:
 buf=b''
 end=time.monotonic()+140
 edited=False; unlocked=False; moved=False
 while time.monotonic()<end:
  try: b=s.recv(8192)
  except socket.timeout: continue
  if not b: break
  log.write(b);buf=(buf+b)[-8192:]
  if b'Use the ^ and v keys' in buf and not edited:
   s.sendall(b'e');buf=b'';edited=True
   time.sleep(.4)
   s.sendall(b'\x01\x0e\x0e\x0e\x05' + boot_arg + b'\x18');moved=True
  if b'Please enter passphrase' in buf and not unlocked:
   s.sendall((p/'disk-unlock.txt').read_bytes().strip()+b'\r');unlocked=True;buf=b''
  if b'login:' in buf: break
 print('grub edited:',moved,'unlock supplied:',unlocked)
