"""Noisy-only pilot timing/CFO on fresh, paired event windows.

The benchmark supplies matched event windows, not sample epochs or clean CFO.
"""
import csv,hashlib,json,os,re,subprocess,sys,time
from datetime import datetime
from pathlib import Path
import numpy as np
from best_common import load,band_noise,quantize,invoke
root=Path(sys.argv[1]);assert not(root/'diversity-results.csv').exists()
sources=json.loads((root/'sources.json').read_text());pairs=[];seen=set()
for a in sorted(sources,key=lambda r:(r['expected_wire_sha256'],r['name'])):
 for b in sorted(sources,key=lambda r:r['name']):
  digest=a['expected_wire_sha256']
  if a['site']>=b['site'] or digest!=b['expected_wire_sha256'] or digest in seen:continue
  delta=abs((datetime.fromisoformat(a['estimated_utc'])-datetime.fromisoformat(b['estimated_utc'])).total_seconds())
  if delta<=2:
   pairs.append((a,b));seen.add(digest);break
 if len(pairs)==4:break
(root/'diversity-pairs.json').write_text(json.dumps([dict(a=a,b=b) for a,b in pairs],indent=2))
if not pairs:
 (root/'diversity-summary.json').write_text(json.dumps(dict(blocked='No common wire body within2s across distinct sites',pairs=0),indent=2));print('No matched fresh transmissions');sys.exit(0)
rows=[];details=[]
for pair,(a,b) in enumerate(pairs):
 clean=[load(root,s) for s in (a,b)];expected=a['expected_wire_sha256'];allowed=set(a['allowed_wire_sha256']+b['allowed_wire_sha256'])
 for snr in (None,-8,-4,0,4):
  for replicate in range(1 if snr is None else 10):
   seed=998000+pair*1000+(0 if snr is None else (snr+8)*20)+replicate;inputs=[];epochs=[];hashes=[]
   for branch,x in enumerate(clean):
    rng=np.random.default_rng(seed*2+branch);jitter=int(rng.integers(-36000,36001));shifted=np.roll(x,jitter);epoch=96000+jitter;epochs.append(epoch)
    if snr is not None:
     noise=band_noise(len(x),rng);noise*=np.sqrt(np.mean(x[96000:329472]**2)/np.mean(noise[epoch:epoch+233472]**2))*10**(-snr/20)
     shifted+=noise
    pcm,audio,_=quantize(shifted);inputs.append(audio);hashes.append(hashlib.sha256(pcm.tobytes()).hexdigest())
   single=[invoke(root,x,'baseline',f'diversity-single-{i}.f32') for i,x in enumerate(inputs)]
   paths=[root/'diversity-a.f32',root/'diversity-b.f32']
   for path,x in zip(paths,inputs):x.astype('<f4').tofile(path)
   start=time.perf_counter();result=subprocess.run([str(root/'prototype/replay'),*map(str,paths)],env=dict(os.environ,SOFT_METHOD='baseline'),capture_output=True,text=True,check=True,timeout=120)
   duration=time.perf_counter()-start;outputs={};unmatched=set()
   for line in result.stdout.splitlines():
    parts=line.split('\t');digest=hashlib.sha256(bytes.fromhex(parts[-1])).hexdigest();outputs[parts[0]]=digest
    if digest not in allowed:unmatched.add(digest)
   candidates=[[],[]]
   for branch,start,hz,score in re.findall(r'ACQ,(\d+),(\d+),([^,\n]+),([^,\n]+)',result.stderr):
    candidates[int(branch)-1].append(dict(start=int(start),hz=float(hz),score=float(score)))
   production=[int(expected in r['wires']) for r in single];same_search=[int(outputs.get(k)==expected) for k in ('B1','B2')];combined=int(outputs.get('C')==expected)
   row=dict(pair=pair,site1=a['site'],site2=b['site'],snr=snr,seed=seed,jitter1=epochs[0]-96000,jitter2=epochs[1]-96000,input1_sha256=hashes[0],input2_sha256=hashes[1],
            stream1=production[0],stream2=production[1],stream_union=int(any(production)),search1=same_search[0],search2=same_search[1],search_union=int(any(same_search)),combined=combined,
            candidates1=len(candidates[0]),candidates2=len(candidates[1]),timing_candidate1=int(any(abs(c['start']-epochs[0])<=256 for c in candidates[0])),
            timing_candidate2=int(any(abs(c['start']-epochs[1])<=256 for c in candidates[1])),unmatched=len(unmatched),seconds=duration)
   rows.append(row)
   if unmatched or (snr is None and not combined):details.append(dict(trial=row,candidates=candidates,unmatched=sorted(unmatched)))
 print('pair done',pair+1,len(pairs),flush=True)
with (root/'diversity-results.csv').open('w') as f:
 w=csv.DictWriter(f,fieldnames=rows[0]);w.writeheader();w.writerows(rows)
summary={}
for snr in (None,-8,-4,0,4):
 a=[r for r in rows if r['snr']==snr]
 summary[str(snr)]=dict(trials=len(a),**{key:sum(r[key] for r in a) for key in ('stream1','stream2','stream_union','search1','search2','search_union','combined','timing_candidate1','timing_candidate2','unmatched')},
  gains_over_stream_union=sum(r['combined']>r['stream_union'] for r in a),losses_vs_stream_union=sum(r['combined']<r['stream_union'] for r in a),
  gains_over_search_union=sum(r['combined']>r['search_union'] for r in a),losses_vs_search_union=sum(r['combined']<r['search_union'] for r in a),seconds=sum(r['seconds'] for r in a))
(root/'diversity-summary.json').write_text(json.dumps(dict(pairs=len(pairs),summary=summary,limitation='Matched event windows supplied; clean timing/CFO not supplied. Not autonomous continuous cross-site event pairing.'),indent=2))
(root/'diversity-unmatched.json').write_text(json.dumps(details,indent=2))
for pattern in ('diversity-*.f32',):
 for path in root.glob(pattern):path.unlink()
print(json.dumps(summary,indent=2))
