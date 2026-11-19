from pathlib import Path
import json, subprocess
import numpy as np
r=Path(Path('/tmp/linkwise-cw01-current').read_text()); a=Path(Path('/tmp/linkwise-acceptance-current').read_text()); p=Path.home()/'Documents/Codex/Linkwise BW500 sensitivity/live'; profile=Path.home()/'Documents/Codex/linkwise-kiwi-field.VtgdnY/profile.json'
with subprocess.Popen([r/'replay-level4',profile],stdin=subprocess.PIPE,stdout=subprocess.PIPE) as s:
 for folder in [p/'linkwise-responder-wasapi',p/'linkwise-caller-wasapi',p/'linkwise-responder-awgn-44',a/'interop-responder',a/'interop-caller']:
  meta=json.loads((folder/'result.json').read_text()); side='caller' if meta['case']=='linkwise-responder' else 'responder'; print(folder.name,side,meta['payload_length'])
  for f in sorted(folder.glob(side+'-??.f32')):
   y=np.fromfile(f,dtype='<f4'); n=len(y)
   if not 201217<=n<=280000:continue
   early=y[:min(n,9600)]; power=np.mean(early[:len(early)//480*480].reshape(-1,480).astype(float)**2,axis=1); start=int(np.argmax(power>.0001))*480
   for coarse in (start,0,max(0,start-1024)):
    s.stdin.write(f'{n} {coarse} KN6MQS\n'.encode()+y.tobytes());s.stdin.flush();out=s.stdout.readline().decode().strip()
    if out!='MISS':break
   print(f.name,n,start,coarse,out)
 s.stdin.close()
