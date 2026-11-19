"""Reproduce the two unexpected outputs from the tracking/clipping screen.

Usage: python adjudicate.py STUDY_ROOT TRACKING_EXPERIMENT
Retains hashes and frame metadata; does not print message bodies.
"""
import csv,hashlib,json,os,subprocess,sys,wave
from pathlib import Path
import numpy as np
study,root=map(Path,sys.argv[1:]);repo=Path(__file__).resolve().parents[2]
prototype=root/'prototype'
main=(prototype/'main.rs').read_text()
main=main.replace('} else {println!("OTHER");}', '''} else {
      print!("OTHER:{}:{}:{}:",f.level,f.samples.start,f.frame.payload.len());
      print!("{:02x}",f.frame.control);for b in &f.frame.payload {print!("{b:02x}");}println!();
   }''')
assert 'OTHER:{}' in main
(prototype/'detail.rs').write_text(main)
deps=repo/'rust/target/release/deps'
cmd=[str(Path.home()/'.cargo/bin/rustc'),'--edition=2024','-O',str(prototype/'detail.rs'),'-L',f'dependency={deps}']
for name in ('rustfft','thiserror','serde','serde_json'):
    lib=max(deps.glob(f'lib{name}-*.rlib'),key=lambda p:p.stat().st_mtime)
    cmd+=['--extern',f'{name}={lib}']
cmd+=['-o',str(prototype/'detail')]
with (root/'detail-build.log').open('w') as f:subprocess.run(cmd,stderr=f,check=True)
unexpected=[r for r in csv.DictReader((root/'challenges.csv').open()) if int(r['unexpected'])]
results=[];temp=root/'adjudicate.f32'
for row in unexpected:
    with wave.open(str(study/f"snr-sweep/fresh-sweep-20261117/clean-{row['source']}.wav")) as f:
        x=np.frombuffer(f.readframes(f.getnframes()),dtype='<i2').reshape(-1,f.getnchannels())[:,0].astype(float)/32768
    n=len(x);rms=np.sqrt(np.mean(x[96000:329472]**2));freq=np.fft.rfftfreq(n,1/48000)
    spectrum=np.fft.rfft(np.random.default_rng(int(row['seed'])).standard_normal(n));spectrum[(freq<300)|(freq>3300)]=0
    noise=np.fft.irfft(spectrum,n=n);noise*=rms/np.sqrt(np.mean(noise[96000:329472]**2))
    signal=x.copy()
    if row['condition']=='echo':signal[96:]+=.6*x[:-96]
    else:assert row['condition']=='noise'
    y=signal+noise;y*=min(1,.9/np.max(np.abs(y)));y=y.astype('<f4')
    assert hashlib.sha256(y.tobytes()).hexdigest()==row['input_sha256']
    y.tofile(temp)
    lines=subprocess.check_output([str(prototype/'detail'),str(temp)],env=dict(os.environ,SOFT_METHOD=row['method']),text=True).splitlines()
    frames=[]
    for line in lines:
        if line.startswith('OTHER:'):
            _,level,start,size,body=line.split(':')
            frames.append(dict(level=int(level),sample=int(start),wire_payload_bytes=int(size),wire_body_sha256=hashlib.sha256(bytes.fromhex(body)).hexdigest(),generic_kiss=False))
        else:frames.append(dict(kiss_sha256=hashlib.sha256(bytes.fromhex(line)).hexdigest(),generic_kiss=True))
    results.append(dict(trial=row,frames=frames))
temp.unlink()
(root/'unexpected-adjudication.json').write_text(json.dumps(results,indent=2))
print(json.dumps(results,indent=2))
