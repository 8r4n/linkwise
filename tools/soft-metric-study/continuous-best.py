"""Regression-check entire fresh recordings, including non-target intervals."""
import json,sys,wave
from pathlib import Path
import numpy as np
from best_common import METHODS,converted_wav,invoke
root=Path(sys.argv[1]);rows=[]
for meta in sorted(root.glob('*/capture.json')):
 originals=json.loads(meta.read_text())['files']
 for original in originals:
  converted=converted_wav(meta.parent,original)
  if not converted.exists() and len(originals)==1:converted=meta.parent/'48k.wav'
  if not converted.exists():continue
  with wave.open(str(converted)) as f:audio=np.frombuffer(f.readframes(f.getnframes()),dtype='<i2').astype('<f4')/32768
  observed={}
  for method in METHODS:
   result=invoke(root,audio,method,'continuous.f32');wires=result.pop('wires');kiss=result.pop('kiss')
   observed[method]=dict(wires=sorted(wires),kiss=sorted(kiss),**result)
  base=set(observed['baseline']['wires'])
  for method in METHODS:
   observed[method]['gains']=sorted(set(observed[method]['wires'])-base);observed[method]['losses']=sorted(base-set(observed[method]['wires']))
  rows.append(dict(capture=meta.parent.name,original=original['name'],original_sha256=original['sha256'],
                   converted_wav=str(converted.relative_to(root)),seconds=len(audio)/48000,methods=observed))
(root/'continuous-validation.json').write_text(json.dumps(rows,indent=2));(root/'continuous.f32').unlink(missing_ok=True)
print(json.dumps([dict(capture=r['capture'],original=r['original'],counts={m:len(v['frames']) for m,v in r['methods'].items()},gated_gains=len(r['methods']['gated']['gains']),gated_losses=len(r['methods']['gated']['losses'])) for r in rows],indent=2))
