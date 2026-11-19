"""Archive compact results and draw the A4 screening figure.

Usage: python report.py SOFT_EXPERIMENT TRACKING_EXPERIMENT
"""
import hashlib,json,shutil,sys
from pathlib import Path
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

soft,tracking=map(Path,sys.argv[1:]);repo=Path(__file__).resolve().parents[2]
out=Path(__file__).resolve().parent/'results';out.mkdir(exist_ok=True)
files={
 'soft-screen.csv':soft/'results.csv','tracking-screen.csv':tracking/'results.csv',
 'soft-challenges.csv':soft/'challenges.csv','tracking-challenges.csv':tracking/'challenges.csv',
 'soft-summary.json':soft/'summary.json','tracking-summary.json':tracking/'summary.json',
 'soft-challenges.json':soft/'challenges.json','tracking-challenges.json':tracking/'challenges.json',
 'bounds.json':soft/'bounds.json','fresh.json':soft/'fresh/results.json',
 'diversity.json':soft/'diversity/summary.json','diversity.csv':soft/'diversity/results.csv',
 'unexpected-adjudication.json':tracking/'unexpected-adjudication.json',
 'soft-protocol.txt':soft/'protocol.txt','tracking-protocol.txt':tracking/'protocol.txt',
 'challenges-protocol.txt':soft/'challenges-protocol.txt','bounds-protocol.txt':soft/'bounds-protocol.txt',
 'diversity-protocol.txt':soft/'diversity/protocol.txt',
}
for name,path in files.items():shutil.copyfile(path,out/name)
provenance={name:hashlib.sha256(path.read_bytes()).hexdigest() for name,path in files.items()}
for label,root in (('soft',soft),('tracking',tracking)):
 for path in sorted((root/'prototype').rglob('*.rs')):
  provenance[label+'/'+str(path.relative_to(root))]=hashlib.sha256(path.read_bytes()).hexdigest()
(out/'sha256.json').write_text(json.dumps(provenance,indent=2))
summaries=[json.loads((p/'summary.json').read_text()) for p in (soft,tracking)]
challenges=[json.loads((p/'challenges.json').read_text())['exact'] for p in (soft,tracking)]
methods=('baseline','tone','symbol','combined','track','clip')
conditions=('noise','tone','colored','impulse','drift','clock','fading','echo')
counts=[]
for method in methods:
 index=int(method in ('track','clip'))
 counts.append([summaries[index]['validation'][method]['exact']]+[challenges[index][c][method] for c in conditions])
counts=np.array(counts);denom=np.array([80]+[20]*8);percent=counts/denom*100
fig=plt.figure(figsize=(297/25.4,210/25.4),facecolor='white')
fig.text(.045,.94,'DSP screening: gains depend on the channel',fontsize=21,weight='bold',color='#183d4a')
fig.text(.045,.895,'Exact payload recovery. Production acquisition, turbo FEC and wire CRC stay fixed.',fontsize=11)
fig.text(.045,.86,'Later corpus: four added-noise levels. Challenges: 0 dB recording / added noise, two source frames, ten seeds each.',fontsize=9)
ax=fig.add_axes([.23,.34,.71,.43]);ax.imshow(percent,cmap='Blues',vmin=0,vmax=100,aspect='auto')
ax.set_xticks(range(9),['Later\ncorpus','Noise\nonly','Narrowband\ninterferer','Colored\nnoise','Impulses','Carrier\ndrift','Clock\n+100 ppm','Fading','2 ms\necho'],fontsize=9)
ax.xaxis.tick_top();ax.tick_params(axis='both',length=0,pad=8)
ax.set_yticks(range(6),['Current baseline','Per-tone noise','Per-symbol confidence','Both noise metrics','Pilot frequency tracking','Impulse clipping'],fontsize=10)
for i in range(6):
 for j in range(9):ax.text(j,i,f'{counts[i,j]}/{denom[j]}',ha='center',va='center',fontsize=10,color='white' if percent[i,j]>=65 else '#183d4a',weight='bold')
ax.set_xticks(np.arange(-.5,9,1),minor=True);ax.set_yticks(np.arange(-.5,6,1),minor=True)
ax.grid(which='minor',color='white',linewidth=2);ax.tick_params(which='minor',length=0)
for spine in ax.spines.values():spine.set_visible(False)
fig.text(.045,.265,'Decision: retain the current receiver. None of these variants improves the later AWGN corpus.',fontsize=12,weight='bold')
fig.text(.045,.225,'Tone weighting helps the interference and echo challenges, but has regressions elsewhere. All six pass clean controls.',fontsize=10)
fig.text(.045,.19,'Tracking and clipping each produce one unexpected CRC-valid non-beacon frame in the challenge set; neither becomes KISS.',fontsize=9,color='#8a2e2e')
fig.text(.045,.145,'Separate feasibility tests',fontsize=11,weight='bold',color='#183d4a')
fig.text(.045,.11,'Known-channel equalizer: 15/20 vs 10/20 echo trials. Ideal aligned audio diversity: 19/20 vs 10/20 per branch.',fontsize=9)
fig.text(.045,.08,'Real two-Kiwi soft combining at added-noise ratio 0 dB: 20/20 vs 17/20 for the better branch; clean timing/CFO supplied.',fontsize=9)
fig.text(.045,.04,'Retrospective screening except fresh reception/diversity. Ratios include original recording noise; these are not absolute RF SNR thresholds.',fontsize=8,color='#465c62')
figure=repo/'lab-notes/figures/soft-metric-study';figure.mkdir(parents=True,exist_ok=True)
fig.savefig(figure/'dsp-screening.pdf');fig.savefig(figure/'dsp-screening.png',dpi=150)
print(figure/'dsp-screening.pdf')
