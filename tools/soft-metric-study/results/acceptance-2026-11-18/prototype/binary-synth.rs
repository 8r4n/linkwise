
mod modem {pub mod coding;pub mod waveform;pub mod bw500;pub mod bw500_level4;pub mod bw500_waveform;pub mod receiver;}
fn main(){
 let args:Vec<_>=std::env::args().collect();let index=args[2].parse::<usize>().unwrap();let mut state=index as u32+17;
 let payload:Vec<u8>=(0..22).map(|i|match index%5{0=>0,1=>255,2=>i as u8,3=>if i%2==0{0}else{255},_=>{state=state.wrapping_mul(1664525).wrapping_add(1013904223);(state>>24) as u8}}).collect();
 let control=[0x00,0x40,0x80][index%3];let frame=modem::bw500::Frame{payload,control};
 print!("{:02x}",frame.control);for b in &frame.payload{print!("{b:02x}");}println!();
 let mut audio=vec![0f32;96000-4096];audio.extend(modem::bw500::synthesize(modem::bw500::Level::Two,&[frame]).unwrap());audio.resize(504000,0.0);
 let bytes:Vec<u8>=audio.iter().flat_map(|x|x.to_le_bytes()).collect();std::fs::write(&args[1],bytes).unwrap();
}
