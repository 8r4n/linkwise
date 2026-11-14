import socket,sys,time
for i in range(80):
 try:
  s=socket.create_connection(('127.0.0.1',2234),timeout=1);break
 except OSError:time.sleep(.1)
else: raise SystemExit(1)
with s,open(sys.argv[1],'ab',buffering=0) as f:
 s.settimeout(None)
 while True:
  b=s.recv(8192)
  if not b:break
  f.write(b)
