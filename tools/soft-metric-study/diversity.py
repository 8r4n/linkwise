"""Actual two-Kiwi soft diversity with clean timing/CFO supplied for both branches.

Usage: python diversity.py STUDY_ROOT SOFT_EXPERIMENT
Shared transmissions are selected by identical CRC-valid generic payload hashes.
This isolates combining; it does not test independent weak-signal synchronization.
"""
import csv,hashlib,json,os,shutil,subprocess,sys,wave
from pathlib import Path
import numpy as np

study,root=map(Path,sys.argv[1:]);repo=Path(__file__).resolve().parents[2]
out=root/'diversity';out.mkdir(exist_ok=False)
(out/'protocol.txt').write_text('''Frozen before scoring. Use simultaneous fresh captures
from Rockport and Kitchener, match equal generic payload hashes in clean decodes.
Supply each clean frame's timing and CFO to impaired trials; no weak acquisition claim.
Estimate CFO on clean pilots over -50..50Hz in2Hz steps. Three decodes per pair:
single branch1, single branch2, equal-weight sum of normalized bit metrics.
Unchanged max-log FEC/interleaver/CRC; no transmitter or wire-format change.
SNR -8,-4,0dB,10 independent added-noise seeds per transmission and SNR.
Band-limit Gaussian noise300-3300Hz; use each recorded frame RMS as reference.
Record known-payload exact results, gains over best branch and oracle union.
Previously unseen captures, but pairing requires clean successful reception.
This selection does not establish a natural weak-signal RF error rate.
''')
prototype=out/'prototype';prototype.mkdir();modem=prototype/'modem';modem.mkdir()
for name in ('bw500','waveform'):shutil.copytree(repo/'rust/src/modem'/name,modem/name)
for name in ('bw500','waveform','coding'):
    shutil.copy(repo/f'rust/src/modem/{name}.rs',modem/f'{name}.rs')
path=modem/'bw500.rs';source=path.read_text()
start=source.index('fn decode_aligned(');end=source.index('    coding::decode(',start)
helper=source[start:end].replace('fn decode_aligned(', 'fn aligned_metrics(',1).replace('-> Option<Frame> {','-> Vec<f64> {',1)+'    coded\n}\n'
source+='\n'+helper+'''
pub fn diversity_metrics(samples: &[f32], clean: &[f32]) -> Vec<f64> {
 let mode=Mode::new(Level::Two);
 let tones=(-25..=25).map(|x|mode.tones_at(x as f64*2.0))
   .max_by(|a,b| reference_score(&mode,a,clean).total_cmp(&reference_score(&mode,b,clean))).unwrap();
 aligned_metrics(&mode,&tones,samples)
}
pub fn diversity_decode(metrics: &[f64]) -> Option<Frame> {
 let mode=Mode::new(Level::Two);
 coding::decode(metrics,&mode.interleaver,mode.rate_third,|bits|mode.unpack(bits))
}
'''
path.write_text(source)
(prototype/'main.rs').write_text('''
mod modem {pub mod coding;pub mod waveform;pub mod bw500;
 pub mod receiver {pub struct ReceivedFrame {pub level:u8,pub samples:std::ops::Range<u64>,pub frame:super::bw500::Frame,}}
}
fn load(p:&str)->Vec<f32>{std::fs::read(p).unwrap().as_chunks::<4>().0.iter().map(|b|f32::from_le_bytes(*b)).collect()}
fn main(){
 let args:Vec<_>=std::env::args().collect();
 let clean1=load(&args[1]);let noisy1=load(&args[2]);let clean2=load(&args[3]);let noisy2=load(&args[4]);
 let a=modem::bw500::diversity_metrics(&noisy1,&clean1);let b=modem::bw500::diversity_metrics(&noisy2,&clean2);
 let combined:Vec<_>=a.iter().zip(&b).map(|(x,y)|x+y).collect();
 for metrics in [&a,&b,&combined]{
  if let Some(frame)=modem::bw500::diversity_decode(metrics){
   print!("{:02x}",frame.control);for byte in frame.payload {print!("{byte:02x}");}println!();
  }else{println!("FAIL");}
 }
}
''')
deps=repo/'rust/target/release/deps'
cmd=[str(Path.home()/'.cargo/bin/rustc'),'--edition=2024','-O',str(prototype/'main.rs'),'-L',f'dependency={deps}']
lib=max(deps.glob('librustfft-*.rlib'),key=lambda p:p.stat().st_mtime)
cmd+=['--extern',f'rustfft={lib}','-o',str(prototype/'replay')]
with (out/'build.log').open('w') as f:subprocess.run(cmd,stderr=f,check=True)
captures=[]
for folder in sorted((root/'fresh').iterdir()):
    if not folder.is_dir():continue
    with wave.open(str(folder/'48k.wav')) as f:audio=np.frombuffer(f.readframes(f.getnframes()),dtype='<i2').astype('<f4')/32768
    temp=out/'full.f32';audio.tofile(temp)
    decoder=study/'snr-sweep/diagnostic-2026-11-17/continuous/replay'
    lines=subprocess.check_output([str(decoder),str(temp)],text=True).splitlines();temp.unlink()
    frames={}
    for line in lines:
        fields=line.split('\t')
        if len(fields)!=5 or fields[1]!='2':continue
        digest=hashlib.sha256(bytes.fromhex(fields[4])).hexdigest()
        frames[digest]=dict(start=int(fields[2]),wire=fields[3])
    captures.append((folder.name,audio,frames))
assert len(captures)==2
shared=sorted(set(captures[0][2])&set(captures[1][2]));assert shared
results=[]
for pair,digest in enumerate(shared):
    clean=[];expected=None
    for branch,(host,audio,frames) in enumerate(captures):
        frame=frames[digest];start=frame['start'];clip=audio[start:start+228*1024]
        assert len(clip)==228*1024
        if expected is None:expected=frame['wire']
        else:assert expected==frame['wire']
        clip.tofile(out/f'clean-{branch}.f32');clean.append(clip)
    paths=[str(out/p) for p in ('clean-0.f32','noisy-0.f32','clean-1.f32','noisy-1.f32')]
    for branch in (0,1):clean[branch].tofile(out/f'noisy-{branch}.f32')
    clean_outputs=subprocess.check_output([str(prototype/'replay'),*paths],text=True).splitlines()
    assert clean_outputs==[expected]*3,clean_outputs
    for snr in (-8,-4,0):
        for replicate in range(10):
            seed=995000+pair*1000+(snr+8)*20+replicate
            for branch,x in enumerate(clean):
                n=len(x);freq=np.fft.rfftfreq(n,1/48000)
                spectrum=np.fft.rfft(np.random.default_rng(seed*2+branch).standard_normal(n));spectrum[(freq<300)|(freq>3300)]=0
                noise=np.fft.irfft(spectrum,n=n);noise*=np.sqrt(np.mean(x.astype(float)**2)/np.mean(noise**2))*10**(-snr/20)
                (x+noise).astype('<f4').tofile(out/f'noisy-{branch}.f32')
            outputs=subprocess.check_output([str(prototype/'replay'),*paths],text=True).splitlines();assert len(outputs)==3
            results.append(dict(pair=pair,kiss_sha256=digest,snr=snr,seed=seed,branch1=int(outputs[0]==expected),branch2=int(outputs[1]==expected),
                                combined=int(outputs[2]==expected),unexpected=sum(o not in ('FAIL',expected) for o in outputs)))
with (out/'results.csv').open('w') as f:
    w=csv.DictWriter(f,fieldnames=results[0]);w.writeheader();w.writerows(results)
summary={str(s):dict(trials=sum(r['snr']==s for r in results),branch1=sum(r['branch1'] for r in results if r['snr']==s),
                   branch2=sum(r['branch2'] for r in results if r['snr']==s),combined=sum(r['combined'] for r in results if r['snr']==s),
                   union=sum(bool(r['branch1'] or r['branch2']) for r in results if r['snr']==s),
                   new_beyond_union=sum(r['combined'] and not(r['branch1'] or r['branch2']) for r in results if r['snr']==s),
                   losses_vs_union=sum(not r['combined'] and bool(r['branch1'] or r['branch2']) for r in results if r['snr']==s)) for s in (-8,-4,0)}
(out/'summary.json').write_text(json.dumps(dict(shared_transmissions=len(shared),per_snr=summary,unexpected=sum(r['unexpected'] for r in results)),indent=2))
for p in out.glob('*.f32'):p.unlink()
print(json.dumps(summary,indent=2))
