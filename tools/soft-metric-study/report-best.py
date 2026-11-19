"""Archive hash-only evidence and draw one A4 chart for the fresh-site retest."""
import hashlib,json,platform,shutil,subprocess,sys
from pathlib import Path
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
root=Path(sys.argv[1]);repo=Path(__file__).resolve().parents[2]
out=repo/'tools/soft-metric-study/results/best-sites-2026-11-18';out.mkdir(parents=True,exist_ok=True)
def sanitize(x):
 if isinstance(x,dict):return {k:sanitize(v) for k,v in x.items() if k not in ('wire','kiss','expected_wire','expected_kiss')}
 if isinstance(x,list):return [sanitize(v) for v in x]
 return x
names=['protocol.txt','protocol-amendment.txt','diversity-extension-protocol.txt','development.csv','development.json',
 'validation.csv','validation-summary.json','validation-unmatched.json','continuous-validation.json','selected-sources.json','captures.json',
 'diversity-results.csv','diversity-summary.json','diversity-pairs.json','diversity-extension.csv','diversity-extension-summary.json',
 'stock-summary.json','stock-counterexample-summary.json','stock-natural-diagnostic-summary.json','unmatched-adjudication.json','source-hashes.json']
for name in names:
 source=root/name
 if source.suffix=='.json':(out/name).write_text(json.dumps(sanitize(json.loads(source.read_text())),indent=2))
 else:shutil.copyfile(source,out/name)
revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()
environment=dict(base_revision=revision,platform=platform.platform(),python=sys.version,numpy=np.__version__,rustc=subprocess.check_output([str(Path.home()/'.cargo/bin/rustc'),'-V'],text=True).strip(),
                 evidence_directory=str(root),production_changed=False,stock_version='VARA HF v4.9.1',waveform='BW500 level2 unchanged',
                 receiver_scope='Framed offline diversity acquisition; continuous singles; no clean timing/CFO in primary diversity',
                 oracle_scope='Separately labelled adaptive diagnostic on already-inspected frames')
(out/'environment.json').write_text(json.dumps(environment,indent=2))
checks={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in root.rglob('*.rs')}
for name in ('prototype/replay','oracle/replay','stock/stock.log','counterexample/stock.log','natural-diagnostic/stock.log'):
 checks[name]=hashlib.sha256((root/name).read_bytes()).hexdigest()
for p in Path(__file__).parent.glob('*.py'):checks['script/'+p.name]=hashlib.sha256(p.read_bytes()).hexdigest()
for p in out.iterdir():
 if p.name!='checksums.json':checks['archive/'+p.name]=hashlib.sha256(p.read_bytes()).hexdigest()
(out/'checksums.json').write_text(json.dumps(checks,indent=2))
v=json.loads((root/'validation-summary.json').read_text());d=json.loads((root/'diversity-summary.json').read_text());e=json.loads((root/'diversity-extension-summary.json').read_text())
fig=plt.figure(figsize=(297/25.4,210/25.4),facecolor='white')
fig.text(.045,.945,'Fresh-site retest: targeted weighting helps; equal combining can hurt',fontsize=18,weight='bold',color='#183d4a')
fig.text(.045,.902,'Westminster T2FD and Warrenton wire installation | 18 baseline receptions, 16 payloads | four 180-second captures',fontsize=10)
fig.text(.045,.864,'Six validation clips represent five payloads. Diversity uses two shared transmissions; no clean epoch or CFO supplied.',fontsize=9)
left=fig.add_axes([.065,.43,.425,.34]);right=fig.add_axes([.57,.43,.38,.34])
conditions=['noise:-8','noise:-4','noise:0','noise:4','tone:0','echo:0'];x=np.arange(6);width=.24
colors=['#718087','#89b6c5','#087e8b']
for i,(method,label,color) in enumerate(zip(('baseline','tone','gated'),('Baseline','Always whiten','Gated fallback'),colors)):
 values=[v['summary'][method][c]['exact'] for c in conditions];bars=left.bar(x+(i-1)*width,values,width,label=label,color=color)
 for bar,value in zip(bars,values):left.text(bar.get_x()+width/2,value+.8,str(value),ha='center',fontsize=8)
left.set_ylim(0,66);left.set_xticks(x,['Noise\n-8 dB','Noise\n-4 dB','Noise\n0 dB','Noise\n+4 dB','Tone\n0 dB','Echo\n0 dB'],fontsize=8)
left.set_ylabel('Exact recoveries / 60 trials',fontsize=9);left.set_title('1. Gated interference weighting',fontsize=12,loc='left',pad=22)
left.legend(loc='upper left',bbox_to_anchor=(0,1.07),ncol=3,fontsize=8,frameon=False)
x=np.arange(4)
for i,(key,label,color) in enumerate([('stream1','Warrenton','#89b6c5'),('stream2','Westminster','#718087'),('combined','Blind equal sum','#c76b35')]):
 values=[d['summary'][str(s)][key] for s in (-8,-4,0,4)];bars=right.bar(x+(i-1)*width,values,width,label=label,color=color)
 for bar,value in zip(bars,values):right.text(bar.get_x()+width/2,value+.4,str(value),ha='center',fontsize=8)
right.set_ylim(0,22);right.set_xticks(x,['-8 dB','-4 dB','0 dB','+4 dB'],fontsize=9);right.set_ylabel('Exact recoveries / 20 trials',fontsize=9)
right.set_yticks([0,5,10,15,20])
right.set_title('2. Diversity with noisy-input acquisition',fontsize=12,loc='left',pad=22)
right.legend(loc='upper left',bbox_to_anchor=(0,1.07),ncol=3,fontsize=8,frameon=False)
for ax in (left,right):
 ax.set_axisbelow(True);ax.yaxis.grid(True,alpha=.2);ax.spines[['top','right']].set_visible(False)
fig.text(.045,.345,'Activation: persistent tone-floor ratio >= 4 in both frame halves; baseline decode first; one-sided weighting only after failure.',fontsize=9)
fig.text(.045,.307,'Regression result: gated decoding preserves all 167/240 AWGN recoveries and all 18 natural baseline receptions.',fontsize=10,weight='bold')
fig.text(.045,.27,'Adoption blocker: one unmatched CRC-valid body (39 bits different), rejected by generic KISS recognition. No production change.',fontsize=9,color='#9a3d2e')
fig.text(.045,.22,'Acquisition diagnostic, new seeds on the same two events:',fontsize=10,weight='bold',color='#183d4a')
fig.text(.045,.183,'At -12 dB: strongest continuous receiver 8/20; noisy-only combination 0/20; supplied clean-calibration combination 10/20.',fontsize=9)
fig.text(.045,.148,'At -8 dB: strongest receiver 20/20; noisy-only combination 3/20; clean-calibration combination 20/20. Oracle is diagnostic only.',fontsize=9)
fig.text(.045,.105,'Stock VARA 4.9.1: 6/6 transport and 6/6 fresh clean controls passed. Matched tone subset: gated 2/4, stock 0/4.',fontsize=9)
fig.text(.045,.068,'All dB values are recorded-burst / added 300-3300 Hz noise ratios, including original recording noise; they are not absolute RF SNR.',fontsize=8)
fig.text(.045,.035,'Primary parameters frozen before fresh scoring. Event windows supplied; independent continuous multi-site pairing remains unimplemented.',fontsize=8)
figure=repo/'lab-notes/figures/best-sites-retest';figure.mkdir(parents=True,exist_ok=True)
fig.savefig(figure/'best-sites-retest.pdf');fig.savefig(figure/'best-sites-retest.png',dpi=150)
checks['figure/best-sites-retest.pdf']=hashlib.sha256((figure/'best-sites-retest.pdf').read_bytes()).hexdigest()
(out/'checksums.json').write_text(json.dumps(checks,indent=2))
print(figure/'best-sites-retest.pdf')
