
mod modem {pub mod coding;pub mod waveform;pub mod bw500;pub mod bw500_level4;pub mod bw500_waveform;pub mod receiver;}
fn load(path: &str) -> Vec<f32> {
    use std::io::Read;
    let raw = if path == "-" {
        let mut bytes = Vec::new();
        std::io::stdin().lock().read_to_end(&mut bytes).unwrap();
        bytes
    } else {
        std::fs::read(path).unwrap()
    };
    assert_eq!(raw.len() % 4, 0, "incomplete float32 PCM sample");
    raw.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect()
}
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
