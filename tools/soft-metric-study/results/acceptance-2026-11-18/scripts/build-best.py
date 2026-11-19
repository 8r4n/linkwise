"""Build frozen research-only gated metrics and noisy-input pilot acquisition.

Usage: python build-best.py EXPERIMENT_ROOT
"""
import hashlib,json,shutil,subprocess,sys
from pathlib import Path
repo=Path(__file__).resolve().parents[2];root=Path(sys.argv[1]);prototype=root/'prototype';prototype.mkdir(exist_ok=False)
modem=prototype/'modem';modem.mkdir()
for name in ('bw500','waveform'):shutil.copytree(repo/'rust/src/modem'/name,modem/name)
for name in ('bw500','waveform','coding','bw500_level4','bw500_waveform','receiver'):shutil.copy(repo/f'rust/src/modem/{name}.rs',modem/f'{name}.rs')
path=modem/'bw500.rs';source=path.read_text();first=source.index('fn decode_aligned(');last=source.index('/// Finds CRC-valid BW500 DATA frames',first)
source=source[:first]+r'''
fn research_median(mut x: Vec<f64>) -> f64 {
 x.sort_by(f64::total_cmp);let n=x.len();
 if n==0 {1e-12} else if n%2==0 {(x[n/2-1]+x[n/2])*0.5} else {x[n/2]}
}
fn research_metrics(mode:&Mode,tones:&[Tone],samples:&[f32],weights:&[f64])->Vec<f64>{
 let mut coded=vec![0.0;mode.channel.len()];let mut channel=mode.channel.as_chunks::<3>().0.iter();
 for (column,chunk) in mode.columns.iter().zip(samples.chunks_exact(mode.symbol)) {
  if let Column::Data(bins)=column {
   let norm=(energy(chunk)*mode.symbol as f64/2.0).max(f64::EPSILON);
   let powers=bins.map(|b|tones[b].power(chunk)/norm*weights[b]);
   for (bit,&position) in channel.next().unwrap().iter().enumerate(){
    let mut best=[0.0_f64;2];for (value,power) in powers.iter().enumerate(){let choice=(value>>(2-bit))&1;best[choice]=best[choice].max(*power);}
    coded[position]=best[1]-best[0];
   }
  }
 }
 coded
}
fn research_floors(mode:&Mode,tones:&[Tone],samples:&[f32])->(Vec<f64>,bool){
 let mut rows=vec![Vec::new();tones.len()];let mut halves=[vec![Vec::new();tones.len()],vec![Vec::new();tones.len()]];
 for (index,(column,chunk)) in mode.columns.iter().zip(samples.chunks_exact(mode.symbol)).enumerate(){
  if let Column::Data(bins)=column{
   let norm=(energy(chunk)*mode.symbol as f64/2.0).max(f64::EPSILON);let half=index*2/mode.columns.len();
   for b in bins {let p=tones[*b].power(chunk)/norm;rows[*b].push(p);halves[half][*b].push(p);}
  }
 }
 let mut peaks=[0usize;2];let mut gate=true;
 for (i,half) in halves.into_iter().enumerate(){
  if half.iter().any(|row|row.len()<16){gate=false;}
  let floors:Vec<_>=half.into_iter().map(research_median).collect();let common=research_median(floors.clone()).max(1e-12);
  let peak=(0..floors.len()).max_by(|&a,&b|floors[a].total_cmp(&floors[b])).unwrap();
  peaks[i]=peak;gate&=floors[peak]/common>=4.0;
 }
 gate&=peaks[0].abs_diff(peaks[1])<=1;
 let mut floors:Vec<_>=rows.into_iter().map(research_median).collect();let common=research_median(floors.clone()).max(1e-12);
 for value in &mut floors {*value=value.max(common*0.1).max(1e-12);}
 (floors,gate)
}
pub static GATE_ATTEMPTS:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);
pub static GATE_ACTIVE:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);
fn decode_aligned(mode:&Mode,tones:&[Tone],samples:&[f32])->Option<Frame>{
 let method=std::env::var("SOFT_METHOD").unwrap_or_else(|_|"baseline".into());
 let decode=|metrics:&[f64]|coding::decode(metrics,&mode.interleaver,mode.rate_third,|bits|mode.unpack(bits));
 let weights=vec![1.0;tones.len()];
 if mode.level!=Level::Two || method=="baseline" {return decode(&research_metrics(mode,tones,samples,&weights));}
 if method=="gated"{
  if let Some(frame)=decode(&research_metrics(mode,tones,samples,&weights)){return Some(frame);}
  GATE_ATTEMPTS.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
 }
 let (floors,gate)=research_floors(mode,tones,samples);
 if method=="gated" && !gate {return None;}
 if method=="gated" {GATE_ACTIVE.fetch_add(1,std::sync::atomic::Ordering::Relaxed);}
 let common=research_median(floors.clone());
 let weights:Vec<_>=floors.iter().map(|f|if method=="gated"{(common/f).clamp(0.1,1.0)}else{common/f}).collect();
 decode(&research_metrics(mode,tones,samples,&weights))
}
pub fn research_branch(audio:&[f32],start:usize,hz:f64)->Vec<f64>{
 let mode=Mode::new(Level::Two);research_metrics(&mode,&mode.tones_at(hz),&audio[start..start+mode.samples_per_frame()],&vec![1.0;mode.span])
}
pub fn research_decode(metrics:&[f64])->Option<Frame>{
 let mode=Mode::new(Level::Two);coding::decode(metrics,&mode.interleaver,mode.rate_third,|bits|mode.unpack(bits))
}
''' +source[last:]
path.write_text(source)
scan=modem/'bw500/scan.rs';scan.write_text(scan.read_text()+r'''
// Offline framed acquisition from impaired inputs only. No expected payload or clean CFO.
pub fn research_candidates(audio:&[f32])->Vec<(usize,f64,f64)>{
 let profile=Profile::new(Level::Two);let mode=&profile.mode;let length=mode.samples_per_frame();
 let mut planner=FftPlanner::new();let mut spectrum=Spectrum::new(&mut planner,mode.symbol,mode.first_bin,mode.span);
 let mut coarse=Vec::new();
 for end in (mode.symbol..=audio.len()).step_by(HOP){
  spectrum.push(&audio[end-mode.symbol..end]);
  if end<length {continue;}
  let scores=spectrum.scores(&profile);
  let offset=(0..5).max_by(|&a,&b|scores[a].total_cmp(&scores[b])).unwrap();
  if scores[offset]>=MIN_LEVEL_TWO_SCORE {coarse.push((end-length,LEVEL_TWO_OFFSETS_HZ[offset] as f64,scores[offset] as f64));}
 }
 coarse.sort_by(|a,b|b.2.total_cmp(&a.2));let mut picked:Vec<(usize,f64,f64)>=Vec::new();
 for (start,hz,_) in coarse {
  if picked.iter().any(|c|c.0.abs_diff(start)<512){continue;}
  let mut best=(start,hz,f64::NEG_INFINITY);
  for residual in [-6.0,-3.0,0.0,3.0,6.0]{
   let tones=mode.tones_at(hz+residual);
   for epoch in (start.saturating_sub(HOP)..=(start+HOP).min(audio.len()-length)).step_by(16){
    let score=reference_score(mode,&tones,&audio[epoch..epoch+length]);if score>best.2{best=(epoch,hz+residual,score);}
   }
  }
  picked.push(best);if picked.len()==4{break;}
 }
 picked.sort_by(|a,b|b.2.total_cmp(&a.2));picked
}
''')
main=r'''
mod modem {pub mod coding;pub mod waveform;pub mod bw500;pub mod bw500_level4;pub mod bw500_waveform;pub mod receiver;}
fn load(path:&str)->Vec<f32>{std::fs::read(path).unwrap().as_chunks::<4>().0.iter().map(|b|f32::from_le_bytes(*b)).collect()}
fn frame(frame:&modem::bw500::Frame){print!("{:02x}",frame.control);for b in &frame.payload{print!("{b:02x}");}}
fn main(){
 let args:Vec<_>=std::env::args().collect();let audio=load(&args[1]);
 if args.len()==2{
  let mut receiver=modem::receiver::Receiver::new().with_pilot_acquisition();
  let report=|e:modem::receiver::Reception|for f in e.frames{
   print!("{}\t{}\t",f.level,f.samples.start);frame(&f.frame);
   if let Some(kiss)=f.kiss_generic_payload(){print!("\t");for b in kiss{print!("{b:02x}");}}println!();
  };
  for chunk in audio.chunks(997){receiver.push(chunk,report).unwrap();}receiver.finish(report);
  use std::sync::atomic::Ordering::Relaxed;
  eprintln!("GATES,{},{}",modem::bw500::GATE_ATTEMPTS.load(Relaxed),modem::bw500::GATE_ACTIVE.load(Relaxed));
 }else{
  let other=load(&args[2]);let a=modem::bw500::scan::research_candidates(&audio);let b=modem::bw500::scan::research_candidates(&other);
  eprintln!("CANDIDATES,{},{}",a.len(),b.len());
  for (branch,candidates) in [&a,&b].into_iter().enumerate(){for &(start,hz,score) in candidates{eprintln!("ACQ,{},{},{},{}",branch+1,start,hz,score);}}
  for (branch,(input,candidates)) in [(&audio,&a),(&other,&b)].into_iter().enumerate(){
   for &(start,hz,score) in candidates {
    let metrics=modem::bw500::research_branch(input,start,hz);
    if let Some(decoded)=modem::bw500::research_decode(&metrics){print!("B{}\t{}\t{}\t{}\t",branch+1,start,hz,score);frame(&decoded);println!();break;}
   }
  }
  let mut pairs:Vec<_>=a.iter().flat_map(|x|b.iter().map(move|y|(*x,*y))).collect();pairs.sort_by(|(a,b),(c,d)|(c.2+d.2).total_cmp(&(a.2+b.2)));
  for ((start,hz,score),(start2,hz2,score2)) in pairs {
   let a=modem::bw500::research_branch(&audio,start,hz);let b=modem::bw500::research_branch(&other,start2,hz2);
   let combined:Vec<_>=a.iter().zip(&b).map(|(x,y)|x+y).collect();
   if let Some(decoded)=modem::bw500::research_decode(&combined){print!("C\t{}\t{}\t{}\t{}\t{}\t{}\t",start,hz,score,start2,hz2,score2);frame(&decoded);println!();break;}
  }
 }
}
'''
(prototype/'main.rs').write_text(main)
deps=repo/'rust/target/release/deps';command=[str(Path.home()/'.cargo/bin/rustc'),'--edition=2024','-O',str(prototype/'main.rs'),'-L',f'dependency={deps}']
for name in ('rustfft','thiserror','serde','serde_json'):
 lib=max(deps.glob(f'lib{name}-*.rlib'),key=lambda p:p.stat().st_mtime);command+=['--extern',f'{name}={lib}']
command+=['-o',str(prototype/'replay')]
with (root/'build.log').open('w') as log:subprocess.run(command,stderr=log,check=True)
(root/'source-hashes.json').write_text(json.dumps({str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in prototype.rglob('*.rs')},indent=2))
print(prototype/'replay')
