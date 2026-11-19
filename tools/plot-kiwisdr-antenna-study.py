#!/usr/bin/env python3
"""Plot selected sufficiently long receiver histories, not causal antenna gains."""
import json
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

repo=Path(__file__).resolve().parents[1]
study=json.loads((repo/'reference/kiwisdr-antenna-study/2026-11-18-summary.json').read_text())
labels={
 'http://g8ure.ddns.net:8079':'Chichester, UK | 80 m dipole',
 'http://sa4bna.hopto.org:8073':'Glava, Sweden | 100 m Beverage*',
 'http://sdr.autreradioautreculture.com:8073':'Tremolat, France | pylon / mini-whip (ambiguous)',
 'http://g8ure.ddns.net:8073':'Chichester, UK | north Beverage',
 'http://oh3aa.ddns.net:18073':'Hameenlinna, Finland | crossed LZ1AQ loops',
 'http://21748.proxy.kiwisdr.com':'Free Union, Virginia | 27.4 m T2FD',
 'http://sdr.hfunderground.com:8073':'Westminster, Maryland | 120 ft T2FD',
 'http://sdr.hfunderground.com:8078':'Westminster, Maryland | 58 ft T2FD',
 'http://sdr.hfunderground.com:8076':'Westminster, Maryland | 500 ft NE Beverage',
 'http://kiwisdr1.sdrutah.org:8073':'Northern Utah | TCI 530 omni',
 'http://kiwisdr.kfsdr.com:8076':'KFS, California | TCI 527B, 135 degrees',
 'http://kiwisdr.kfsdr.com:8074':'KFS, California | TCI 532, 278 degrees',
 'http://kiwisdr.kfsdr.com:8075':'KFS, California | TCI 532, 222 degrees',
 'http://rx2.wa2zkd.net:8073':'Rockport, Maine | Pixel loop',
 'http://ve3nsv.ddns.net:8074':'Kitchener, Ontario | 80 m doublet',
 'http://zl2ba.proxy.kiwisdr.com':'Masterton, New Zealand | broadband flat-top',
 'http://alg.twrmon.net:8073':'Gqeberha, South Africa | Wellbrook loop',
}
rows=[r for r in study['examples'] if r['url'] in labels and r.get('history',{}).get('span_hours',0)>=20 and r['history']['window_samples']>=12]
rows.sort(key=lambda r:r['history']['bands']['hf']['median'],reverse=True)
fig=plt.figure(figsize=(297/25.4,210/25.4),facecolor='white')
fig.text(.04,.94,'Good KiwiSDR installations use several antenna types',fontsize=18,weight='bold',color='#183d4a')
fig.text(.04,.895,'Selected receiver systems: median reported HF spectrum score over each latest available 24-hour window.',fontsize=10)
fig.text(.04,.86,'Directory: 18 Nov 2026, 01:39 UTC. A higher score is not proof of better reception of the same VarAC transmission.',fontsize=9)
ax=fig.add_axes([.46,.245,.37,.55]);values=[r['history']['bands']['hf']['median'] for r in rows]
colors=['#176b7a' if 'TCI' not in labels[r['url']] else '#82939a' for r in rows]
ax.barh(range(len(rows)),values,color=colors,height=.65)
ax.set_yticks(range(len(rows)),[labels[r['url']] for r in rows],fontsize=8.5)
ax.invert_yaxis();ax.set_xlim(0,46);ax.set_xticks([0,10,20,30,40]);ax.set_xlabel('95th percentile minus median spectrum level (dB)',fontsize=9)
ax.xaxis.grid(True,alpha=.2);ax.set_axisbelow(True);ax.tick_params(axis='y',length=0)
for spine in ax.spines.values():spine.set_visible(False)
for i,v in enumerate(values):ax.text(v+.5,i,f'{v:g}',va='center',fontsize=9)
ax.text(1.13,1.045,'40 m',transform=ax.transAxes,fontsize=9,ha='center')
ax.text(1.32,1.045,'20 m',transform=ax.transAxes,fontsize=9,ha='center')
for i,r in enumerate(rows):
 for x,key in ((1.13,'40m'),(1.32,'20m')):
  band=r['history']['bands'].get(key)
  text=f"{band['median']:g}" if band else '--'
  ax.text(x,i,text,transform=ax.get_yaxis_transform(),ha='center',va='center',fontsize=9)
fig.text(.04,.165,'Read this as a shortlist of installations, not a global antenna ranking or a controlled A/B comparison.',fontsize=11,weight='bold')
fig.text(.04,.125,'Histories shown span 20-24 hours with at least 12 observations; sampling times and intervals differ. -- = band data unavailable.',fontsize=9)
fig.text(.04,.09,'*Glava reports 1.8-32 MHz; other plotted HF summaries use 1.8-30 MHz. Local-time records are not synchronized UTC trials.',fontsize=9)
fig.text(.04,.055,'Antenna labels are operator reports. Site noise, propagation, orientation, filtering and active antenna selection remain confounders.',fontsize=9)
out=repo/'lab-notes/figures/kiwisdr-antennas';out.mkdir(parents=True,exist_ok=True)
fig.savefig(out/'antenna-installations.pdf');fig.savefig(out/'antenna-installations.png',dpi=150)
