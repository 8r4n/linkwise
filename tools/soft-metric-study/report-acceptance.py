"""Archive acceptance evidence; distinguish development from prospective RF holdout."""
import hashlib,json,shutil,sys
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np
root=Path(sys.argv[1]);repo=Path(__file__).resolve().parents[2]
out=repo/'tools/soft-metric-study/results/acceptance-2026-11-18';out.mkdir(parents=True,exist_ok=True)
def sanitize(x):
 if isinstance(x,dict):return {k:sanitize(v) for k,v in x.items() if k not in ('wire','kiss','expected_wire','expected_kiss')}
 if isinstance(x,list):return [sanitize(v) for v in x]
 return x
for prefix in ('','validation2/','stable-check/'):
 for name in ('protocol.txt','acceptance-selection.json','acceptance-selection-v2.json','trace-summary.json','acceptance-development.csv','validation.csv','validation-summary.json','validation-unmatched.json','continuous-validation.json','captures.json','selected-sources.json','stock-summary.json','binary-regression.json','binary-regression-stable.json','structured-negatives.json','structured-negatives-stable.json','structured-negatives-holdout.json','interop-summary.json','unconfirmed-source-trace.json'):
  p=root/prefix/name
  if not p.exists():continue
  dest=out/prefix/name;dest.parent.mkdir(parents=True,exist_ok=True)
  if p.suffix=='.json':dest.write_text(json.dumps(sanitize(json.loads(p.read_text())),indent=2))
  else:shutil.copyfile(p,dest)
snap=out/'scripts';snap.mkdir(exist_ok=True)
for p in Path(__file__).parent.glob('*.py'):shutil.copyfile(p,snap/p.name)
for directory in ('prototype','trace'):
 for p in (root/directory).rglob('*.rs'):
  dest=out/directory/p.relative_to(root/directory);dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(p,dest)
v=json.loads((root/'validation2/validation-summary.json').read_text());n=json.loads((root/'structured-negatives-holdout.json').read_text())['summary'];s=json.loads((root/'validation2/stock-summary.json').read_text())
fig=plt.figure(figsize=(297/25.4,210/25.4));fig.text(.06,.94,'Interference recovery with stricter frame acceptance',fontsize=19,weight='bold',color='#183d4a')
fig.text(.06,.89,'Later RF holdout: Westminster + Warrenton, 18 November 2026, 12:59 UTC | 14 receptions; selected 6 clips / 3 payloads',fontsize=10)
ax=fig.add_axes([.09,.41,.85,.38]);conditions=['noise:-12','noise:-8','noise:-4','noise:0','noise:4','tone:0','echo:0'];x=np.arange(7)
for i,(m,label,color) in enumerate([('baseline','Production baseline','#7c898e'),('gated','Original gated fallback','#dca46e'),('stable','Two-iteration acceptance','#087e8b')]):
 vals=[v['summary'][m][c]['exact'] for c in conditions];bars=ax.bar(x+(i-1)*.24,vals,.24,label=label,color=color)
 for b,value in zip(bars,vals):ax.text(b.get_x()+.12,value+.8,str(value),ha='center',fontsize=8)
ax.set_ylim(0,68);ax.set_ylabel('Exact payload recoveries / 60 trials');ax.set_xticks(x,['Noise\n-12 dB','Noise\n-8 dB','Noise\n-4 dB','Noise\n0 dB','Noise\n+4 dB','CW + noise\n0 dB','Echo + noise\n0 dB']);ax.legend(ncol=3,frameon=False,loc='upper left',bbox_to_anchor=(0,1.12));ax.spines[['top','right']].set_visible(False);ax.set_axisbelow(True);ax.yaxis.grid(alpha=.2)
fig.text(.06,.32,'Acceptance control: noise-only DATA with genuine pilots + persistent CW; 100 independently seeded windows.',fontsize=10,weight='bold')
fig.text(.06,.275,'CRC-valid bodies accepted: '+ ' | '.join(f"{m}: {n[m]['crc_frames']}" for m in ('baseline','gated','stable')),fontsize=11)
fig.text(.06,.225,f"Clean: all Linkwise variants 6/6; stock VARA {s['new_clean_pass']}/{s['new_clean']}; transport controls {s['transport_controls_pass']}/{s['transport_controls']}.",fontsize=10)
fig.text(.06,.18,'Ordinary mixed VARA transfers passed in both roles. Synthetic binary weighted-path checks: 20/20.',fontsize=10)
fig.text(.06,.135,'New rule applies only to weighted fallback: skip raw hard-bit CRC; require identical CRC-valid consecutive turbo outputs.',fontsize=9)
fig.text(.06,.095,'Six clips are a small selected corpus; trials reuse recordings. Repeated CRC iterations are correlated. No production change.',fontsize=9)
fig.text(.06,.055,'dB = recorded burst / added 300-3300 Hz noise, not absolute RF SNR. CW and echo are synthetic impairments.',fontsize=9)
figure=repo/'lab-notes/figures/acceptance-retest';figure.mkdir(parents=True,exist_ok=True);fig.savefig(figure/'acceptance-retest.pdf');fig.savefig(figure/'acceptance-retest.png',dpi=120)
(out/'environment.json').write_text(json.dumps(dict(evidence_directory=str(root),production_changed=False,scope='Development, first RF batch, later prospective holdout kept separate; no95percent RF threshold or global antenna ranking.'),indent=2))
checks={str(p.relative_to(out)):hashlib.sha256(p.read_bytes()).hexdigest() for p in out.rglob('*') if p.is_file() and p.name!='checksums.json'}
for p in (root/'prototype').rglob('*'):
 if p.is_file():checks['local-prototype/'+str(p.relative_to(root/'prototype'))]=hashlib.sha256(p.read_bytes()).hexdigest()
for p in root.rglob('stock.log'):
 checks['local-evidence/'+str(p.relative_to(root))]=hashlib.sha256(p.read_bytes()).hexdigest()
(out/'checksums.json').write_text(json.dumps(checks,indent=2))
print(json.dumps({m:{c:r['exact'] for c,r in rows.items()} for m,rows in v['summary'].items()},indent=2))
