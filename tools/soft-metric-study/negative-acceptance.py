"""Structured erasure negatives: known pilots, DATA replaced entirely by noise.

Deliberately challenges acceptance after acquisition, unlike quiet noise-only controls.
"""
import csv,json,os,subprocess,sys
from pathlib import Path
import numpy as np
from best_common import band_noise,quantize,invoke
root=Path(sys.argv[1]);temp=root/'negative-template.f32'
methods=tuple(os.environ.get('NEGATIVE_METHODS','baseline,gated,iterated').split(','))
tag=os.environ.get('NEGATIVE_TAG','');assert tag in ('','-stable','-holdout');seed_base=int(os.environ.get('NEGATIVE_SEED_BASE','2120000'))
subprocess.run([str(root/'prototype/binary-synth'),str(temp),'0'],capture_output=True,check=True)
template=np.fromfile(temp,dtype='<f4').astype(float);temp.unlink();rms=np.sqrt(np.mean(template[96000:329472]**2));pilots={(104*k+93)//11 for k in range(24)}
rows=[]
for index in range(100):
 noise=band_noise(len(template),np.random.default_rng(seed_base+index));noise*=rms/np.sqrt(np.mean(noise[96000:329472]**2));signal=template.copy()
 for column in range(228):
  if column not in pilots:
   start=96000+column*1024;signal[start:start+1024]=noise[start:start+1024]
 signal+=2*rms*np.sqrt(2)*np.sin(2*np.pi*1453*np.arange(len(signal))/48000+.3)
 _,audio,_=quantize(signal)
 for method in methods:
  result=invoke(root,audio,method,'negative-input.f32');rows.append(dict(seed=seed_base+index,method=method,frames=len(result['frames']),generic_kiss=len(result['kiss']),gate_active=result['gate_active']))
with (root/f'structured-negatives{tag}.csv').open('w') as f:
 w=csv.DictWriter(f,fieldnames=rows[0]);w.writeheader();w.writerows(rows)
summary={m:dict(windows=100,seconds=1050,crc_frames=sum(r['frames'] for r in rows if r['method']==m),generic_kiss=sum(r['generic_kiss'] for r in rows if r['method']==m),gate_attempts=sum(r['gate_active'] for r in rows if r['method']==m)) for m in methods}
(root/f'structured-negatives{tag}.json').write_text(json.dumps(dict(summary=summary,scope='Synthetic true pilot schedule with noise-only DATA and persistent CW; no valid DATA frame transmitted. Not a natural RF false-frame rate.'),indent=2));(root/'negative-input.f32').unlink(missing_ok=True)
print(json.dumps(summary,indent=2))
