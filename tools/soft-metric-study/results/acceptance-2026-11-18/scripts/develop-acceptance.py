"""Choose acceptance on old data only, before opening fresh capture decodes."""
import csv,json,sys
from datetime import datetime,timezone
from pathlib import Path
from best_common import load,trial,invoke
root,old=map(Path,sys.argv[1:]);sources=json.loads((old/'selected-sources.json').read_text())
assert not(root/'acceptance-selection.json').exists()
(root/'development-protocol.txt').write_text(datetime.now(timezone.utc).isoformat()+': Compare baseline,old gate,FEC-iterated acceptance,and two-consecutive-full-iteration acceptance on the previous6 sources/60 interfered trials plus6 clean controls. New recordings remain unopened. Eligible rule must reject the known unmatched case, produce no unmatched development outputs, retain at least49/60 tone recoveries and6/6 clean. Maximize tone exact count; tie prefer minimally restrictive iterated CRC. If none eligible, report no candidate. No application/beacon-format filters.\n')
rows=[]
for index,source in enumerate(sources):
 for condition,replicates in [('clean',[0]),('tone',range(10))]:
  for replicate in replicates:
   seed=996000+index*1000+160+replicate
   audio=load(old,source) if condition=='clean' else trial(old,source,'tone',0,seed)[1]
   for method in ('baseline','gated','iterated','stable'):
    r=invoke(root,audio,method,'development.f32');rows.append(dict(source=source['name'],condition=condition,seed=seed,method=method,
     exact=int(source['expected_wire_sha256'] in r['wires']),unmatched=len(r['wires']-set(source['allowed_wire_sha256'])),seconds=r['seconds']))
summary={m:dict(tone_exact=sum(r['exact'] for r in rows if r['method']==m and r['condition']=='tone'),clean=sum(r['exact'] for r in rows if r['method']==m and r['condition']=='clean'),
              unmatched=sum(r['unmatched'] for r in rows if r['method']==m),seconds=sum(r['seconds'] for r in rows if r['method']==m)) for m in ('baseline','gated','iterated','stable')}
eligible=[m for m in ('iterated','stable') if summary[m]['tone_exact']>=49 and summary[m]['clean']==6 and summary[m]['unmatched']==0]
selected=max(eligible,key=lambda m:(summary[m]['tone_exact'],m=='iterated')) if eligible else None
with (root/'acceptance-development.csv').open('w') as f:
 w=csv.DictWriter(f,fieldnames=rows[0]);w.writeheader();w.writerows(rows)
(root/'acceptance-selection.json').write_text(json.dumps(dict(selected=selected,summary=summary),indent=2));(root/'development.f32').unlink(missing_ok=True)
print(json.dumps(dict(selected=selected,summary=summary),indent=2))
