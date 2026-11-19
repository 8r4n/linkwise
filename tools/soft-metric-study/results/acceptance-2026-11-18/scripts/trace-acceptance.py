"""Trace every turbo iteration on the previous unmatched frame, preserving first acceptance."""
import csv,hashlib,json,os,shutil,subprocess,sys,wave
from pathlib import Path
import numpy as np
from best_common import trial,load
root,old=map(Path,sys.argv[1:]);repo=Path(__file__).resolve().parents[2]
prototype=root/'trace';shutil.copytree(old/'prototype',prototype,ignore=shutil.ignore_patterns('replay'))
path=prototype/'modem/coding.rs';source=path.read_text();needle='pub(super) fn decode<T>('
assert source.count(needle)==1;source=source.replace(needle,'fn decode_inner<T>(',1)
source+=r'''
pub(super) fn decode<T>(coded:&[f64],interleaver:&[usize],rate_third:bool,check:impl Fn(&[u8])->Option<T>)->Option<T>{
 if std::env::var("FEC_TRACE").is_err(){return decode_inner(coded,interleaver,rate_third,check);}
 use std::cell::{Cell,RefCell};use std::sync::atomic::{AtomicUsize,Ordering};
 static CALLS:AtomicUsize=AtomicUsize::new(0);let id=CALLS.fetch_add(1,Ordering::Relaxed);
 let stage=Cell::new(0usize);let accepted=RefCell::new(None);let previous=RefCell::new(Vec::<u8>::new());
 let _:Option<()>=decode_inner(coded,interleaver,rate_third,|bits|{
  let iteration=stage.get();stage.set(iteration+1);let frame=check(bits);let valid=frame.is_some();
  let first=accepted.borrow().is_none();if first{*accepted.borrow_mut()=frame;}
  let fingerprint=bits.iter().fold(0xcbf29ce484222325u64,|h,b|(h^u64::from(*b)).wrapping_mul(0x100000001b3));
  let encoded=encode(bits,interleaver,rate_third);let total:f64=coded.iter().map(|x|x.abs()).sum();
  let agreement:f64=encoded.iter().zip(coded).map(|(b,x)|(2.0*f64::from(*b)-1.0)*x).sum::<f64>()/total.max(1e-12);
  let changes=bits.iter().zip(previous.borrow().iter()).filter(|(a,b)|a!=b).count();
  eprintln!("FEC,{},{},{},{:016x},{:.9},{}",id,iteration,valid,fingerprint,agreement,changes);
  *previous.borrow_mut()=bits.to_vec();None
 });
 accepted.into_inner()
}
''';path.write_text(source)
deps=repo/'rust/target/release/deps';cmd=[str(Path.home()/'.cargo/bin/rustc'),'--edition=2024','-O',str(prototype/'main.rs'),'-L',f'dependency={deps}']
for name in ('rustfft','thiserror','serde','serde_json'):
 lib=max(deps.glob(f'lib{name}-*.rlib'),key=lambda p:p.stat().st_mtime);cmd+=['--extern',f'{name}={lib}']
cmd+=['-o',str(prototype/'replay')]
with (root/'trace-build.log').open('w') as log:subprocess.run(cmd,stderr=log,check=True)
sources=json.loads((old/'selected-sources.json').read_text());bad=json.loads((old/'validation-unmatched.json').read_text())[0]['trial'];source=next(s for s in sources if s['name']==bad['source'])
positive=next(r for r in csv.DictReader((old/'validation.csv').open()) if r['condition']=='tone' and r['method']=='gated' and r['exact']=='1')
positive_source=next(s for s in sources if s['name']==positive['source'])
cases=[('counterexample',source,bad['condition'],bad['snr'],bad['seed']),('interfered-success-control',positive_source,'tone',0,int(positive['seed'])),('clean-control',source,'clean',None,0)]
results=[]
for label,source,condition,snr,seed in cases:
 if condition=='clean':audio=load(old,source)
 else:_,audio,_=trial(old,source,condition,snr,seed)
 path=root/'trace.f32';audio.astype('<f4').tofile(path)
 result=subprocess.run([str(prototype/'replay'),str(path)],env=dict(os.environ,SOFT_METHOD='gated',FEC_TRACE='1'),capture_output=True,text=True,check=True)
 (root/(label+'-fec.log')).write_text(result.stderr)
 frames=[]
 for line in result.stdout.splitlines():
  parts=line.split('\t');frames.append(dict(level=int(parts[0]),start=int(parts[1]),wire_sha256=hashlib.sha256(bytes.fromhex(parts[2])).hexdigest(),generic_kiss=len(parts)==4))
 valid=[line for line in result.stderr.splitlines() if line.startswith('FEC,') and ',true,' in line]
 results.append(dict(case=label,frames=frames,valid_iterations=valid))
path.unlink();(root/'trace-summary.json').write_text(json.dumps(results,indent=2));print(json.dumps(results,indent=2))
