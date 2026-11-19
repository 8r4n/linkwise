"""A4 summary of separate receiver experiments; no pooled sensitivity claim."""
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
from matplotlib.patches import Patch
out=Path(__file__).resolve().parents[2]/'lab-notes/figures/modem-improvements';out.mkdir(parents=True,exist_ok=True)
f=plt.figure(figsize=(297/25.4,210/25.4))
f.text(.055,.93,'Linkwise: measured decoding improvements',fontsize=22,weight='bold',color='#183d4a')
f.text(.055,.875,'Separate experiments: exact payload recoveries before and after each change',fontsize=12)
a=f.add_axes([.33,.40,.59,.36])
for y,b,c,n,m,color in [(1,15,75,'3/20','15/20','#087e8b'),(0,8/60*100,100,'8/60','60/60','#cf7a32')]:
 a.barh(y+.12,b,.20,color='#879398');a.barh(y-.12,c,.20,color=color)
 for yy,v,t in [(y+.12,b,n),(y-.12,c,m)]:a.text(v+2,yy,f'{v:.0f}% ({t})',va='center',fontsize=12,weight='bold')
a.set(xlim=(0,120),ylim=(-.5,1.5),yticks=[],xticks=[0,25,50,75,100],xticklabels=['0%','25%','50%','75%','100%'],xlabel='Exact recovery rate on the specified test');a.spines[['top','right','left']].set_visible(False);a.set_axisbelow(True);a.xaxis.grid(alpha=.18)
f.legend(handles=[Patch(color='#879398',label='Before'),Patch(color='#087e8b',label='Merged change'),Patch(color='#cf7a32',label='Experimental change')],loc='upper left',bbox_to_anchor=(.32,.825),ncol=3,frameon=False,fontsize=10)
for y,title,status,detail,col in [(.67,'Weak-signal acquisition','MERGED','Pilot threshold: 0.35 to 0.20\nRetained corpus; -8 dB added noise','#087e8b'),(.495,'CW interference recovery','EXPERIMENTAL','Tone weighting + stricter acceptance\nLater holdout: 6 clips / 3 payloads','#b25d22')]:
 f.text(.055,y,title,fontsize=13,weight='bold',color='#183d4a');f.text(.055,y-.035,status,fontsize=10,weight='bold',color=col);f.text(.055,y-.078,detail,fontsize=10,linespacing=1.6)
f.text(.055,.275,'Payload correctness',fontsize=12,weight='bold',color='#183d4a')
f.text(.055,.235,'Seven-bit unpacking fix: 11 matched receptions produced identical KISS bytes to stock VARA.',fontsize=11)
f.text(.055,.18,'Latest candidate safeguards',fontsize=12,weight='bold',color='#183d4a')
f.text(.055,.12,'Preserved 14/14 natural receptions; 0 accepted bodies in 100 structured negative windows.\nOrdinary mixed-stock transfers passed in both roles; impaired mixed-session qualification remains open.',fontsize=10,linespacing=1.7)
f.text(.055,.04,'Different experiments must not be pooled. Seeded trials reuse recordings. Noise ratios are not absolute RF SNR.\nThe experimental fallback is not in production; the transmitted waveform and framing remain unchanged.',fontsize=9,color='#52666e',linespacing=1.6)
f.savefig(out/'modem-improvements.pdf');f.savefig(out/'modem-improvements.png',dpi=110)
