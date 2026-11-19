"""Regression-check entire fresh recordings, including non-target intervals."""
import hashlib,json,sys,wave
from pathlib import Path
import numpy as np
from best_common import METHODS,invoke
root=Path(sys.argv[1]);rows=[]
for meta in sorted(root.glob('*/capture.json')):
 converted=meta.parent/'48k.wav'
 if not converted.exists():continue
 with wave.open(str(converted)) as f:audio=np.frombuffer(f.readframes(f.getnframes()),dtype='<i2').astype('<f4')/32768
 observed={}
 for method in METHODS:
  result=invoke(root,audio,method,'continuous.f32');wires=result.pop('wires');kiss=result.pop('kiss')
  observed[method]=dict(wires=sorted(wires),kiss=sorted(kiss),**result)
 base=set(observed['baseline']['wires'])
 for method in METHODS:
  observed[method]['gains']=sorted(set(observed[method]['wires'])-base);observed[method]['losses']=sorted(base-set(observed[method]['wires']))
 rows.append(dict(capture=meta.parent.name,seconds=len(audio)/48000,methods=observed))
(root/'continuous-validation.json').write_text(json.dumps(rows,indent=2));(root/'continuous.f32').unlink(missing_ok=True)
print(json.dumps([dict(capture=r['capture'],counts={m:len(v['frames']) for m,v in r['methods'].items()},gated_gains=len(r['methods']['gated']['gains']),gated_losses=len(r['methods']['gated']['losses'])) for r in rows],indent=2))
