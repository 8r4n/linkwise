"""Validate stock replay completion, input identity, clean controls and exact KISS sets."""
import base64,csv,hashlib,json,os,re,sys
from collections import defaultdict
from pathlib import Path
import numpy as np
import wave
from best_common import METHODS,invoke
root=Path(sys.argv[1]);batch=root/(sys.argv[2] if len(sys.argv)>2 else 'stock')
manifest=json.loads((batch/'manifest.json').read_text());log=(batch/'stock.log').read_text(encoding='utf-8-sig')
assert 'REPLAY COMPLETE' in log and 'transcript end' in log.lower()
assert all(f'CONNECTED {port}' in log for port in (8360,8361,8160,8370,8371,8170))
completed={name for name,_ in re.findall(r'^END (\S+) position=(\d+) length=\2\s*$',log,re.M)}
assert completed=={r['name'] for r in manifest}
hashes=dict(re.findall(r'^HASH (\S+) ([0-9A-Fa-f]+)\s*$',(batch/'guest-hashes.log').read_text(encoding='utf-8-sig'),re.M))
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
   payload=re.sub(rb'\xdb([\xdc\xdd])',lambda m:b'\xc0' if m[1]==b'\xdc' else b'\xdb',frame[1:]);stock[name].add(hashlib.sha256(payload).hexdigest())
validation=list(csv.DictReader((root/'validation.csv').open()));rows=[]
for row in manifest:
 path=batch/(row['name']+'.wav');assert row['wav_sha256']==hashes[row['name']].lower()==hashlib.sha256(path.read_bytes()).hexdigest()
 with wave.open(str(path)) as f:
  assert f.getnchannels()==2 and f.getframerate()==48000 and f.getsampwidth()==2
  pcm=np.frombuffer(f.readframes(f.getnframes()),dtype='<i2').reshape(-1,2);assert np.array_equal(pcm[:,0],pcm[:,1]);audio=pcm[:,0].astype('<f4')/32768
 if row['kind']=='impaired':
  assert hashlib.sha256(pcm[:,0].tobytes()).hexdigest()==row['input_sha256']
  counterparts=[r for r in validation if r['source']==row['source'] and r['condition']==row['condition'] and float(r['snr'])==row['snr'] and int(r['seed'])==row['seed']]
  assert len(counterparts)==len(METHODS) and all(r['input_sha256']==row['input_sha256'] for r in counterparts)
  linkwise={r['method']:int(r['kiss_exact']) for r in counterparts}
 else:
  linkwise={m:int(row['expected_sha256'] in invoke(root,audio,m,'stock-score.f32')['kiss']) for m in METHODS}
 rows.append(dict(name=row['name'],kind=row['kind'],source=row.get('source'),site=row.get('site'),condition=row.get('condition'),snr=row.get('snr'),seed=row.get('seed'),
                  stock_exact=int(row['expected_sha256'] in stock[row['name']]),stock_additional=len(stock[row['name']]-{row['expected_sha256']}),stock_hashes=sorted(stock[row['name']]),**linkwise))
controls=[r for r in rows if r['kind']=='transport_control'];clean=[r for r in rows if r['kind']=='source_clean']
summary=dict(version='VARA HF v4.9.1',transport_controls_pass=sum(r['stock_exact'] and not r['stock_additional'] for r in controls),transport_controls=len(controls),
             comparator_qualified=all(r['stock_exact'] and not r['stock_additional'] for r in controls),new_clean_pass=sum(r['stock_exact'] for r in clean),new_clean=len(clean),rows=rows,cells={})
noise_snrs=tuple(map(int,os.environ.get('STUDY_NOISE_SNRS','-8,-4,0,4').split(',')))
for condition,snr in [('noise',s) for s in noise_snrs]+[('tone',0),('echo',0)]:
 a=[r for r in rows if r['kind']=='impaired' and r['condition']==condition and r['snr']==snr]
 if a:summary['cells'][f'{condition}:{snr}']=dict(trials=len(a),**{m:sum(r[m] for r in a) for m in ('stock_exact',*METHODS)},stock_additional=sum(r['stock_additional'] for r in a))
output=root/('stock-summary.json' if batch.name=='stock' else 'stock-'+batch.name+'-summary.json');output.write_text(json.dumps(summary,indent=2));(root/'stock-score.f32').unlink(missing_ok=True)
print(json.dumps({k:v for k,v in summary.items() if k!='rows'},indent=2))
