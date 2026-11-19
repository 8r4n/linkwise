"""End-to-end binary PHY regression; this is Linkwise synthesis, not stock generation."""
import csv,hashlib,json,os,subprocess,sys
from pathlib import Path
import numpy as np
from best_common import band_noise,quantize,invoke
root=Path(sys.argv[1]);repo=Path(__file__).resolve().parents[2];prototype=root/'prototype'
tone_factor=float(os.environ.get('BINARY_TONE_FACTOR','1'))
methods=tuple(os.environ.get('BINARY_METHODS','baseline,gated,iterated').split(','));tag=os.environ.get('BINARY_TAG','');assert tag in ('','-stable')
(prototype/'binary-synth.rs').write_text(r'''
mod modem {pub mod coding;pub mod waveform;pub mod bw500;pub mod bw500_level4;pub mod bw500_waveform;pub mod receiver;}
fn main(){
 let args:Vec<_>=std::env::args().collect();let index=args[2].parse::<usize>().unwrap();let mut state=index as u32+17;
 let payload:Vec<u8>=(0..22).map(|i|match index%5{0=>0,1=>255,2=>i as u8,3=>if i%2==0{0}else{255},_=>{state=state.wrapping_mul(1664525).wrapping_add(1013904223);(state>>24) as u8}}).collect();
 let control=[0x00,0x40,0x80][index%3];let frame=modem::bw500::Frame{payload,control};
 print!("{:02x}",frame.control);for b in &frame.payload{print!("{b:02x}");}println!();
 let mut audio=vec![0f32;96000-4096];audio.extend(modem::bw500::synthesize(modem::bw500::Level::Two,&[frame]).unwrap());audio.resize(504000,0.0);
 let bytes:Vec<u8>=audio.iter().flat_map(|x|x.to_le_bytes()).collect();std::fs::write(&args[1],bytes).unwrap();
}
''')
deps=repo/'rust/target/release/deps';cmd=[str(Path.home()/'.cargo/bin/rustc'),'--edition=2024','-O',str(prototype/'binary-synth.rs'),'-L',f'dependency={deps}']
for name in ('rustfft','thiserror','serde','serde_json'):
 lib=max(deps.glob(f'lib{name}-*.rlib'),key=lambda p:p.stat().st_mtime);cmd+=['--extern',f'{name}={lib}']
cmd+=['-o',str(prototype/'binary-synth')]
with (root/'binary-build.log').open('w') as log:subprocess.run(cmd,stderr=log,check=True)
rows=[];temp=root/'binary-source.f32'
for index in range(20):
 expected=subprocess.check_output([str(prototype/'binary-synth'),str(temp),str(index)],text=True).strip();expected=hashlib.sha256(bytes.fromhex(expected)).hexdigest()
 clean=np.fromfile(temp,dtype='<f4').astype(float);rms=np.sqrt(np.mean(clean[96000:329472]**2))
 for condition in ('noise','tone'):
  rng=np.random.default_rng(2110000+index);noise=band_noise(len(clean),rng);noise*=rms/np.sqrt(np.mean(noise[96000:329472]**2));signal=clean+noise
  if condition=='tone':signal+=tone_factor*rms*np.sqrt(2)*np.sin(2*np.pi*1453*np.arange(len(clean))/48000+.3)
  _,audio,_=quantize(signal)
  for method in methods:
   result=invoke(root,audio,method,'binary-input.f32')
   rows.append(dict(case=index,condition=condition,method=method,exact=int(expected in result['wires']),unmatched=len(result['wires']-{expected}),
                    generic_kiss=len(result['kiss']),gate_active=result['gate_active']))
with (root/f'binary-regression{tag}.csv').open('w') as f:
 w=csv.DictWriter(f,fieldnames=rows[0]);w.writeheader();w.writerows(rows)
summary={c:{m:dict(exact=sum(r['exact'] for r in rows if r['condition']==c and r['method']==m),unmatched=sum(r['unmatched'] for r in rows if r['condition']==c and r['method']==m),gates=sum(r['gate_active'] for r in rows if r['condition']==c and r['method']==m)) for m in methods} for c in ('noise','tone')}
(root/f'binary-regression{tag}.json').write_text(json.dumps(dict(trials_per_cell=20,tone_rms_factor=tone_factor,summary=summary,scope='Arbitrary binary PHY payloads synthesized by Linkwise; not a stock-generated fixture or an ARQ session qualification'),indent=2))
for p in (temp,root/'binary-input.f32'):p.unlink(missing_ok=True)
print(json.dumps(summary,indent=2))
