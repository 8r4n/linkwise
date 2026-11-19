"""Fixed exploratory channel challenges, not ITU/Watterson qualification.

Usage: python challenges.py STUDY_ROOT EXISTING_EXPERIMENT_DIRECTORY
"""
import csv
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import wave
import numpy as np

study, root = map(Path, sys.argv[1:])
assert not (root/'challenges-protocol.txt').exists()
(root/'challenges-protocol.txt').write_text('''Frozen after the original soft-metric screen, before challenge scoring.
Two Nov17 clean off-air clips; 10 seeds per condition and source = 20 trials.
All use 0 dB recording/added 300-3300Hz noise over seconds2-6.864.
Conditions: noise alone; narrowband tone at1453Hz, RMS equal to burst;
colored noise with Gaussian amplitude peak5x at1460Hz, sigma70Hz;
impulses (0.5% samples, random polarity, amplitude10x burst RMS);
linear carrier drift -10 to+10Hz across clip via analytic-signal mixer;
clock mismatch +100ppm by linear interpolation;
fading envelope .55+.45*cos(2pi*t), random phase;
two-path echo .6 amplitude,2ms delay.
Seed993000+replicate shared across conditions; no per-condition metric tuning.
Baseline/tone/symbol/combined unchanged. Retrospective exploratory conditions,
not independent RF validation or an ITU-R F.1487 implementation.
Common gain prevents clipping; no new WAV corpus retained; record input hashes.
''')
methods=tuple(os.environ.get('SOFT_METHODS','baseline,tone,symbol,combined').split(','))
assert set(methods)<={'baseline','tone','symbol','combined','track','clip'}
conditions=('noise','tone','colored','impulse','drift','clock','fading','echo')
rows=[]
temp=root/'challenge.f32'
batch=study/'snr-sweep/fresh-sweep-20261117'
manifest=json.loads((batch/'manifest.json').read_text())
for source in (0,1):
    name=f'clean-{source}'
    row=next(r for r in manifest if r['name']==name)
    wav=batch/(name+'.wav')
    assert hashlib.sha256(wav.read_bytes()).hexdigest()==row['wav_sha256']
    with wave.open(str(wav)) as f:
        x=np.frombuffer(f.readframes(f.getnframes()),dtype='<i2').reshape(-1,f.getnchannels())[:,0].astype(float)/32768
    n=len(x);t=np.arange(n)/48000
    active=slice(96000,329472)
    rms=np.sqrt(np.mean(x[active]**2))
    spectrum=np.fft.fft(x);h=np.zeros(n);h[0]=1;h[1:(n+1)//2]=2
    if n%2==0:h[n//2]=1
    analytic=np.fft.ifft(spectrum*h)
    for seed in range(993000,993010):
        rng=np.random.default_rng(seed)
        freqs=np.fft.rfftfreq(n,1/48000)
        noise_spectrum=np.fft.rfft(rng.standard_normal(n));noise_spectrum[(freqs<300)|(freqs>3300)]=0
        noise=np.fft.irfft(noise_spectrum,n=n);noise*=rms/np.sqrt(np.mean(noise[active]**2))
        for condition in conditions:
            signal=x.copy();added=noise.copy()
            if condition=='tone':signal+=rms*np.sqrt(2)*np.sin(2*np.pi*1453*t+.3)
            elif condition=='colored':
                added=np.fft.irfft(noise_spectrum*(1+4*np.exp(-.5*((freqs-1460)/70)**2)),n=n)
                added*=rms/np.sqrt(np.mean(added[active]**2))
            elif condition=='impulse':
                mask=rng.random(n)<.005
                signal[mask]+=10*rms*rng.choice([-1,1],mask.sum())
            elif condition=='drift':
                phase=2*np.pi*(-10*t+10*t*t/t[-1])
                signal=(analytic*np.exp(1j*phase)).real
            elif condition=='clock':signal=np.interp(np.arange(n)*1.0001,np.arange(n),x,right=0)
            elif condition=='fading':signal*=.55+.45*np.cos(2*np.pi*t+rng.uniform(0,2*np.pi))
            elif condition=='echo':signal[96:]+=.6*x[:-96]
            y=signal+added;y*=min(1,.9/np.max(np.abs(y)));y=y.astype('<f4');y.tofile(temp)
            digest=hashlib.sha256(y.tobytes()).hexdigest()
            for method in methods:
                start=time.perf_counter()
                lines=subprocess.check_output([str(root/'prototype/replay'),str(temp)],env=dict(os.environ,SOFT_METHOD=method),text=True,timeout=120).splitlines()
                hashes={hashlib.sha256(bytes.fromhex(line)).hexdigest() if line!='OTHER' else 'OTHER' for line in lines}
                rows.append(dict(source=source,condition=condition,seed=seed,method=method,input_sha256=digest,
                                 exact=int(row['expected_sha256'] in hashes),unexpected=len(hashes-{row['expected_sha256']}),seconds=time.perf_counter()-start))
    print('source complete',source,flush=True)
temp.unlink()
with (root/'challenges.csv').open('w') as f:
    w=csv.DictWriter(f,fieldnames=rows[0]);w.writeheader();w.writerows(rows)
summary={c:{m:sum(r['exact'] for r in rows if r['condition']==c and r['method']==m) for m in methods} for c in conditions}
(root/'challenges.json').write_text(json.dumps(dict(trials_per_cell=20,exact=summary,unexpected=sum(r['unexpected'] for r in rows)),indent=2))
print(json.dumps(summary,indent=2))
