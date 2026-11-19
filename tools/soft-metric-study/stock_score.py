"""Score the separately collected stock startup/clean-control qualification.

Usage: python stock_score.py STOCK_BATCH SOFT_EXPERIMENT
Full capture references are Linkwise observations, not transmitted truth.
"""
import base64,hashlib,json,os,re,subprocess,sys,wave
from collections import defaultdict
from pathlib import Path
import numpy as np

batch,soft=map(Path,sys.argv[1:])
manifest=json.loads((batch/'manifest.json').read_text())
log=(batch/'stock.log').read_text(encoding='utf-8-sig')
assert 'REPLAY COMPLETE' in log and 'transcript end' in log.lower()
assert all(f'CONNECTED {port}' in log for port in (8360,8361,8160,8370,8371,8170))
hashes=dict(re.findall(r'^HASH (\S+) ([0-9A-Fa-f]+)\s*$',(batch/'guest-hashes.log').read_text(encoding='utf-8-sig'),re.M))
completed={name for name,_ in re.findall(r'^END (\S+) position=(\d+) length=\2\s*$',log,re.M)}
assert completed=={r['name'] for r in manifest}
streams=defaultdict(bytearray)
for line in log.splitlines():
 if line.startswith('EVENT '):
  _,name,port,_,data=line.split(maxsplit=4);streams[name,port].extend(base64.b64decode(data,validate=True))
assert any(b'VERSION VARA HF v4.9.1' in s for s in streams.values())
stock=defaultdict(set)
for (name,port),stream in streams.items():
 if port not in ('8160','8170'):continue
 for frame in bytes(stream).split(b'\xc0')[1:-1]:
  if frame.startswith(b'\x02'):
   payload=re.sub(rb'\xdb([\xdc\xdd])',lambda m:b'\xc0' if m[1]==b'\xdc' else b'\xdb',frame[1:])
   stock[name].add(hashlib.sha256(payload).hexdigest())
results=[];temp=batch/'qualification.f32'
for row in manifest:
 name=row['name'];wav=batch/(name+'.wav')
 assert hashes[name].lower()==row['wav_sha256']==hashlib.sha256(wav.read_bytes()).hexdigest()
 with wave.open(str(wav)) as f:
  assert f.getsampwidth()==2 and f.getframerate()==48000
  pcm=np.frombuffer(f.readframes(f.getnframes()),dtype='<i2').reshape(-1,f.getnchannels())
  assert pcm.shape[1]==2 and np.array_equal(pcm[:,0],pcm[:,1])
  audio=pcm[:,0].astype('<f4')/32768
 known=row.get('known_truth',True)
 expected={row['expected_sha256']} if known else set(row['expected_hashes'])
 if known:
  audio.tofile(temp)
  lines=subprocess.check_output([str(soft/'prototype/replay'),str(temp)],env=dict(os.environ,SOFT_METHOD='baseline'),text=True,timeout=120).splitlines()
  linkwise={hashlib.sha256(bytes.fromhex(line)).hexdigest() if line!='OTHER' else 'OTHER' for line in lines}
 else:linkwise=expected
 results.append(dict(name=name,known_truth=known,expected=sorted(expected),linkwise=sorted(linkwise),stock=sorted(stock[name]),
                     stock_matches=len(stock[name]&expected),stock_missing=sorted(expected-stock[name]),stock_additional=sorted(stock[name]-expected),
                     clean_pass=bool(known and stock[name]==expected==linkwise)))
if temp.exists():temp.unlink()
controls=[r for r in results if r['known_truth']]
summary=dict(version='VARA HF v4.9.1',startup_seconds=15,controls_pass=sum(r['clean_pass'] for r in controls),controls=len(controls),
             qualified=all(r['clean_pass'] for r in controls),rows=results,
             limitation='Fresh references are Linkwise decoded observations, not an independently known transmitted-frame denominator. Qualification applies to this replay run only; no claim that startup timing caused prior misses.')
(batch/'qualification.json').write_text(json.dumps(summary,indent=2))
print(json.dumps(summary,indent=2))
