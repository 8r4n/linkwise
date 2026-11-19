use linkwise::modem::bw500_level4;
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let bytes: Vec<u8> = (0..args[1].len()).step_by(2).map(|i| u8::from_str_radix(&args[1][i..i+2], 16).unwrap()).collect();
    let bits = bw500_level4::encode(&bytes.try_into().unwrap()).expect("valid archived CRC");
    std::fs::write(&args[2], bits).unwrap();
}
