"""Build minimal FEC-confirmed and stricter stable-CRC gated fallback candidates."""
import hashlib,json,shutil,subprocess,sys
from pathlib import Path
root,old=map(Path,sys.argv[1:]);repo=Path(__file__).resolve().parents[2];prototype=root/'prototype'
shutil.copytree(old/'prototype',prototype,ignore=shutil.ignore_patterns('replay'))
path=prototype/'modem/bw500.rs';source=path.read_text();assert source.count('method=="gated"')>=4
source=source.replace('method=="gated"','matches!(method.as_str(),"gated"|"iterated"|"stable")')
needle=' decode(&research_metrics(mode,tones,samples,&weights))\n}\npub fn research_branch'
assert source.count(needle)==1
source=source.replace(needle,''' let metrics=research_metrics(mode,tones,samples,&weights);
 if method=="iterated" || method=="stable" {
  coding::decode_confirmed(&metrics,&mode.interleaver,mode.rate_third,|bits|mode.unpack(bits),method=="stable")
 }else{decode(&metrics)}
}
pub fn research_branch''')
path.write_text(source)
path=prototype/'modem/coding.rs';path.write_text(path.read_text()+r'''
// Only the new weighted fallback calls this. Baseline decoding is unchanged.
// Repeated CRC observations are correlated, not independent checksums.
pub(super) fn decode_confirmed<T>(coded:&[f64],interleaver:&[usize],rate_third:bool,check:impl Fn(&[u8])->Option<T>,stable:bool)->Option<T>{
 use std::cell::{Cell,RefCell};let stage=Cell::new(0usize);let previous=RefCell::new(None::<Vec<u8>>);
 decode(coded,interleaver,rate_third,|bits|{
  let iteration=stage.get();stage.set(iteration+1);
  if iteration==0{return None;} // Raw systematic hard decisions are not FEC confirmation.
  let frame=check(bits);
  if !stable{return frame;}
  if let Some(frame)=frame{
   let equal=previous.borrow().as_deref()==Some(bits);*previous.borrow_mut()=Some(bits.to_vec());
   if equal{return Some(frame);}
  }else{*previous.borrow_mut()=None;}
  None
 })
}
''')
deps=repo/'rust/target/release/deps';cmd=[str(Path.home()/'.cargo/bin/rustc'),'--edition=2024','-O',str(prototype/'main.rs'),'-L',f'dependency={deps}']
for name in ('rustfft','thiserror','serde','serde_json'):
 lib=max(deps.glob(f'lib{name}-*.rlib'),key=lambda p:p.stat().st_mtime);cmd+=['--extern',f'{name}={lib}']
cmd+=['-o',str(prototype/'replay')]
with (root/'build.log').open('w') as log:subprocess.run(cmd,stderr=log,check=True)
(root/'source-hashes.json').write_text(json.dumps({str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in prototype.rglob('*.rs')},indent=2))
print('Built baseline, gated, iterated and stable research variants')
