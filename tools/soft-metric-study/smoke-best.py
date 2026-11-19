"""Screen frozen gated policy on the old development corpus before fresh scoring."""
import csv,hashlib,json,os,re,subprocess,sys,wave
from pathlib import Path
import numpy as np
root=Path(sys.argv[1]);study=Path.home()/'Documents/Codex/Linkwise VOACAP/2026-11-15-main-retest.sajUbP'
batch=study/'snr-sweep/sweep';rows=[];temp=root/'smoke.f32'
for row in json.loads((batch/'manifest.json').read_text()):
 wav=batch/(row['name']+'.wav');assert hashlib.sha256(wav.read_bytes()).hexdigest()==row['wav_sha256']
 with wave.open(str(wav)) as f:
  pcm=np.frombuffer(f.readframes(f.getnframes()),dtype='<i2').reshape(-1,f.getnchannels());audio=pcm[:,0].astype('<f4')/32768
 audio.tofile(temp)
 for method in ('baseline','tone','gated'):
  result=subprocess.run([str(root/'prototype/replay'),str(temp)],env=dict(os.environ,SOFT_METHOD=method),capture_output=True,text=True,check=True)
  hashes=set()
  for line in result.stdout.splitlines():
   fields=line.split('\t');hashes.add(hashlib.sha256(bytes.fromhex(fields[3])).hexdigest() if len(fields)==4 else 'OTHER')
  attempts,active=map(int,re.search(r'GATES,(\d+),(\d+)',result.stderr).groups())
  rows.append(dict(name=row['name'],snr=row['nominal_snr_db'],method=method,exact=int(row['expected_sha256'] in hashes),unexpected=len(hashes-{row['expected_sha256']}),gate_attempts=attempts,gate_active=active))
temp.unlink()
with (root/'development.csv').open('w') as f:
 w=csv.DictWriter(f,fieldnames=rows[0]);w.writeheader();w.writerows(rows)
summary={m:dict(exact=sum(r['exact'] for r in rows if r['method']==m and r['snr'] is not None),clean=sum(r['exact'] for r in rows if r['method']==m and r['snr'] is None),unexpected=sum(r['unexpected'] for r in rows if r['method']==m),gates=sum(r['gate_active'] for r in rows if r['method']==m)) for m in ('baseline','tone','gated')}
(root/'development.json').write_text(json.dumps(summary,indent=2));print(json.dumps(summary,indent=2))
