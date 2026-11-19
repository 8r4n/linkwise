"""Adaptive lower-SNR diagnostic with a separately labelled clean-calibration oracle."""
import csv,hashlib,json,os,shutil,subprocess,sys
from datetime import datetime,timezone
from pathlib import Path
import numpy as np
from best_common import load,band_noise,quantize,invoke
root=Path(sys.argv[1]);repo=Path(__file__).resolve().parents[2]
assert not(root/'diversity-extension.csv').exists()
protocol=datetime.now(timezone.utc).isoformat()+': Adaptive diagnostic after the primary run found the strong branch20/20 even at-8dB. Same2 event pairs and unchanged blind algorithm. New seeds; ratios-16,-12,-8dB;ten seeds per pair. Compare production singles, noisy-only acquisition/combination, and explicitly supplied clean calibration. Oracle selects a CRC-valid clean candidate without expected-payload feedback, then supplies its epoch plus injected jitter and clean CFO to noisy metric extraction. Diagnostic oracle, not a deployable result. Additional trials on already-inspected fresh recordings, not independent frame validation.\n'
(root/'diversity-extension-protocol.txt').write_text(protocol)
oracle=root/'oracle';shutil.copytree(root/'prototype',oracle,ignore=shutil.ignore_patterns('replay'))
p=oracle/'modem/bw500.rs';p.write_text(p.read_text()+r'''
pub fn oracle_metrics(audio:&[f32],clean:&[f32],jitter:i64)->Option<Vec<f64>>{
 for (start,hz,_) in scan::research_candidates(clean){
  if research_decode(&research_branch(clean,start,hz)).is_some(){
   let shifted=start as i64+jitter;if shifted<0 || shifted as usize+233472>audio.len(){return None;}
   return Some(research_branch(audio,shifted as usize,hz));
  }
 }
 None
}
''')
(oracle/'main.rs').write_text(r'''
mod modem {pub mod coding;pub mod waveform;pub mod bw500;pub mod bw500_level4;pub mod bw500_waveform;pub mod receiver;}
fn load(p:&str)->Vec<f32>{std::fs::read(p).unwrap().as_chunks::<4>().0.iter().map(|b|f32::from_le_bytes(*b)).collect()}
fn main(){
 let args:Vec<_>=std::env::args().collect();let c1=load(&args[1]);let a1=load(&args[2]);let j1=args[3].parse::<i64>().unwrap();let c2=load(&args[4]);let a2=load(&args[5]);let j2=args[6].parse::<i64>().unwrap();
 let a=modem::bw500::oracle_metrics(&a1,&c1,j1);let b=modem::bw500::oracle_metrics(&a2,&c2,j2);
 let combined=match (&a,&b){(Some(a),Some(b))=>Some(a.iter().zip(b).map(|(x,y)|x+y).collect::<Vec<_>>()),_=>None};
 for (label,metrics) in [("O1",a),("O2",b),("OC",combined)]{
  if let Some(metrics)=metrics{if let Some(frame)=modem::bw500::research_decode(&metrics){print!("{}\t{:02x}",label,frame.control);for byte in frame.payload{print!("{byte:02x}");}println!();}}
 }
}
''')
deps=repo/'rust/target/release/deps';cmd=[str(Path.home()/'.cargo/bin/rustc'),'--edition=2024','-O',str(oracle/'main.rs'),'-L',f'dependency={deps}']
for name in ('rustfft','thiserror','serde','serde_json'):
 lib=max(deps.glob(f'lib{name}-*.rlib'),key=lambda p:p.stat().st_mtime);cmd+=['--extern',f'{name}={lib}']
cmd+=['-o',str(oracle/'replay')]
with (root/'oracle-build.log').open('w') as f:subprocess.run(cmd,stderr=f,check=True)
pairs=json.loads((root/'diversity-pairs.json').read_text());rows=[]
def decode_hashes(stdout):return {s.split('\t')[0]:hashlib.sha256(bytes.fromhex(s.split('\t')[-1])).hexdigest() for s in stdout.splitlines()}
for index,pair in enumerate(pairs):
 sources=[pair['a'],pair['b']];clean=[load(root,s) for s in sources];expected=sources[0]['expected_wire_sha256'];allowed=set(sources[0]['allowed_wire_sha256']+sources[1]['allowed_wire_sha256'])
 cleanpaths=[root/'oracle-clean-0.f32',root/'oracle-clean-1.f32'];paths=[root/'oracle-noisy-0.f32',root/'oracle-noisy-1.f32']
 for path,x in zip(cleanpaths,clean):x.astype('<f4').tofile(path)
 for snr in (-16,-12,-8):
  for replicate in range(10):
   seed=1000000+index*1000+(snr+16)*20+replicate;inputs=[];jitters=[]
   for branch,x in enumerate(clean):
    rng=np.random.default_rng(seed*2+branch);j=int(rng.integers(-36000,36001));jitters.append(j);epoch=96000+j
    noise=band_noise(len(x),rng);noise*=np.sqrt(np.mean(x[96000:329472]**2)/np.mean(noise[epoch:epoch+233472]**2))*10**(-snr/20)
    _,audio,_=quantize(np.roll(x,j)+noise);inputs.append(audio)
   singles=[invoke(root,x,'baseline',f'extension-single-{b}.f32') for b,x in enumerate(inputs)]
   for path,x in zip(paths,inputs):x.astype('<f4').tofile(path)
   blind=subprocess.run([str(root/'prototype/replay'),*map(str,paths)],env=dict(os.environ,SOFT_METHOD='baseline'),capture_output=True,text=True,check=True,timeout=120)
   supplied=subprocess.run([str(oracle/'replay'),str(cleanpaths[0]),str(paths[0]),str(jitters[0]),str(cleanpaths[1]),str(paths[1]),str(jitters[1])],capture_output=True,text=True,check=True,timeout=120)
   a=decode_hashes(blind.stdout);b=decode_hashes(supplied.stdout)
   rows.append(dict(pair=index,snr=snr,seed=seed,stream1=int(expected in singles[0]['wires']),stream2=int(expected in singles[1]['wires']),
                    blind1=int(a.get('B1')==expected),blind2=int(a.get('B2')==expected),blind_combined=int(a.get('C')==expected),
                    oracle1=int(b.get('O1')==expected),oracle2=int(b.get('O2')==expected),oracle_combined=int(b.get('OC')==expected),
                    blind_unmatched=len(set(a.values())-allowed),oracle_unmatched=len(set(b.values())-allowed)))
with (root/'diversity-extension.csv').open('w') as f:
 w=csv.DictWriter(f,fieldnames=rows[0]);w.writeheader();w.writerows(rows)
summary={str(s):dict(trials=sum(r['snr']==s for r in rows),**{k:sum(r[k] for r in rows if r['snr']==s) for k in ('stream1','stream2','blind1','blind2','blind_combined','oracle1','oracle2','oracle_combined','blind_unmatched','oracle_unmatched')}) for s in (-16,-12,-8)}
(root/'diversity-extension-summary.json').write_text(json.dumps(summary,indent=2))
for pattern in ('oracle-*.f32','extension-*.f32'):
 for path in root.glob(pattern):path.unlink()
print(json.dumps(summary,indent=2))
