"""Fresh, frozen gated-metric validation with no parameter fitting."""
import csv,hashlib,json,os,sys
from pathlib import Path
import numpy as np
from best_common import METHODS,load,select_sources,trial,invoke,band_noise
root=Path(sys.argv[1]);assert not(root/'validation.csv').exists()
sources=select_sources(root);assert len({s['expected_kiss_sha256'] for s in sources})>=2
noise_snrs=tuple(map(int,os.environ.get('STUDY_NOISE_SNRS','-8,-4,0,4').split(',')))
seed_base=int(os.environ.get('STUDY_SEED_BASE','996000'))
conditions=[('noise',s) for s in noise_snrs]+[('tone',0),('echo',0)]
(root/'selected-sources.json').write_text(json.dumps(sources,indent=2));rows=[];details=[]
for index,source in enumerate(sources):
 for condition,snr,seeds in [('clean',None,[0])]+[(c,s,range(10)) for c,s in conditions]:
  for replicate in seeds:
   seed=seed_base+index*1000+(0 if snr is None else (snr+8)*20)+replicate
   if condition=='clean':
    audio=load(root,source);meta=dict(input_sha256=hashlib.sha256(np.rint(audio*32768).astype('<i2').tobytes()).hexdigest(),recording_added_noise_ratio_db=None,common_gain=1.0)
   else:_,audio,meta=trial(root,source,condition,snr,seed)
   for method in METHODS:
    result=invoke(root,audio,method);wires=result.pop('wires');kiss=result.pop('kiss');frames=result.pop('frames')
    row=dict(source=source['name'],site=source['site'],condition=condition,snr=snr,seed=seed,method=method,
             exact=int(source['expected_wire_sha256'] in wires),kiss_exact=int(source['expected_kiss_sha256'] in kiss),
             unmatched=len(wires-set(source['allowed_wire_sha256'])),**result,**meta)
    rows.append(row)
    if row['unmatched']:details.append(dict(trial=row,frames=frames,expected=source['allowed_wire_sha256']))
 print('source done',source['site'],index+1,len(sources),flush=True)
with (root/'validation.csv').open('w') as f:
 w=csv.DictWriter(f,fieldnames=rows[0]);w.writeheader();w.writerows(rows)
summary={}
baseline={(r['source'],r['condition'],r['snr'],r['seed']):r['exact'] for r in rows if r['method']=='baseline'}
for method in METHODS:
 summary[method]={}
 for condition,snr in [('clean',None)]+conditions:
  a=[r for r in rows if r['method']==method and r['condition']==condition and r['snr']==snr]
  summary[method][f'{condition}:{snr}']=dict(exact=sum(r['exact'] for r in a),trials=len(a),unmatched=sum(r['unmatched'] for r in a),
    gains=sum(r['exact']>baseline[r['source'],r['condition'],r['snr'],r['seed']] for r in a),losses=sum(r['exact']<baseline[r['source'],r['condition'],r['snr'],r['seed']] for r in a),
    gate_active=sum(r['gate_active'] for r in a),seconds=sum(r['seconds'] for r in a),per_site={site:sum(r['exact'] for r in a if r['site']==site) for site in {r['site'] for r in a}})
controls=[]
for condition in ('gaussian','tone'):
 rng=np.random.default_rng(997001);audio=band_noise(60*48000,rng);audio*=.035/np.sqrt(np.mean(audio**2))
 if condition=='tone':audio+=.2*np.sin(2*np.pi*1453*np.arange(len(audio))/48000)
 for method in METHODS:
  result=invoke(root,audio,method);controls.append(dict(condition=condition,method=method,exposure_seconds=60,crc_frames=len(result['frames']),gate_active=result['gate_active'],seconds=result['seconds']))
(root/'validation-summary.json').write_text(json.dumps(dict(sources=len(sources),distinct_payloads=len({s['expected_kiss_sha256'] for s in sources}),summary=summary,controls=controls),indent=2))
(root/'validation-unmatched.json').write_text(json.dumps(details,indent=2))
(root/'input.f32').unlink(missing_ok=True)
print(json.dumps(summary,indent=2))
