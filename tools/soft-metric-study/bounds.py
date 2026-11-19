"""Oracle channel/diversity headroom, NOT implemented field receivers.

Usage: python bounds.py STUDY_ROOT EXISTING_EXPERIMENT_DIRECTORY
"""
import hashlib,json,os,subprocess,sys,wave
from pathlib import Path
import numpy as np

study,root=map(Path,sys.argv[1:])
assert not (root/'bounds-protocol.txt').exists()
(root/'bounds-protocol.txt').write_text('''Exploratory bounds, frozen before scoring.
Two Nov17 clean clips,10 seeds each. Noise300-3300Hz,0dB recording/added noise
over seconds2-6.864. Echo .6 amplitude,2ms delay. Compare baseline with known
channel inverse H*/(|H|^2+.1); channel supplied by generator, NOT estimated.
Zero pad to twice clip length to reduce circular boundary effects.
Diversity: average two perfectly sample/phase/gain-aligned copies with
independent added noise (-4dB recording/added noise per branch).
This idealized audio combination is NOT soft diversity of actual KiwiSDRs.
Both are feasibility bounds requiring estimation/alignment work before deployment.
Baseline continuous acquisition .20,FEC,CRC and exact payload checks unchanged.
''')
batch=study/'snr-sweep/fresh-sweep-20261117';manifest=json.loads((batch/'manifest.json').read_text())
rows=[];temp=root/'bound.f32'
for source in (0,1):
    row=next(r for r in manifest if r['name']==f'clean-{source}')
    with wave.open(str(batch/(row['name']+'.wav'))) as f:
        x=np.frombuffer(f.readframes(f.getnframes()),dtype='<i2').reshape(-1,f.getnchannels())[:,0].astype(float)/32768
    n=len(x);active=slice(96000,329472);rms=np.sqrt(np.mean(x[active]**2))
    for seed in range(994000,994010):
        noises=[]
        for branch in (0,1):
            rng=np.random.default_rng(seed*2+branch);freq=np.fft.rfftfreq(n,1/48000)
            spectrum=np.fft.rfft(rng.standard_normal(n));spectrum[(freq<300)|(freq>3300)]=0
            noise=np.fft.irfft(spectrum,n=n);noise*=rms/np.sqrt(np.mean(noise[active]**2));noises.append(noise)
        echo=x.copy();echo[96:]+=.6*x[:-96];echo+=noises[0]
        size=n*2;freq=np.fft.rfftfreq(size,1/48000);h=1+.6*np.exp(-2j*np.pi*freq*.002)
        equalized=np.fft.irfft(np.fft.rfft(echo,n=size)*h.conj()/(np.abs(h)**2+.1),n=size)[:n]
        gain=10**(4/20)
        variants=dict(echo=echo,oracle_equalizer=equalized,branch1=x+gain*noises[0],branch2=x+gain*noises[1],oracle_diversity=x+gain*(noises[0]+noises[1])/2)
        for method,y in variants.items():
            y=y*min(1,.9/np.max(np.abs(y)));y.astype('<f4').tofile(temp)
            lines=subprocess.check_output([str(root/'prototype/replay'),str(temp)],env=dict(os.environ,SOFT_METHOD='baseline'),text=True,timeout=120).splitlines()
            hashes={hashlib.sha256(bytes.fromhex(line)).hexdigest() if line!='OTHER' else 'OTHER' for line in lines}
            rows.append(dict(source=source,seed=seed,method=method,exact=int(row['expected_sha256'] in hashes),unexpected=len(hashes-{row['expected_sha256']})))
temp.unlink()
summary={method:sum(r['exact'] for r in rows if r['method']==method) for method in variants}
(root/'bounds.json').write_text(json.dumps(dict(trials_per_method=20,exact=summary,unexpected=sum(r['unexpected'] for r in rows),trials=rows),indent=2))
print(summary)
