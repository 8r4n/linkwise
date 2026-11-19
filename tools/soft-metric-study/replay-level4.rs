//! Framed BW500 level-4 replay for bench experiments. The caller supplies the
//! coarse burst window and the known session caller for DATA parsing.

use linkwise::modem::{bw500_waveform::Waveform, data};
use std::{
    env, fs,
    io::{self, BufRead, Read, Write},
};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    let waveform = Waveform::from_json(&fs::read_to_string(&args[1])?)?;
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let mut line = String::new();
    while input.read_line(&mut line)? != 0 {
        let fields: Vec<_> = line.split_whitespace().collect();
        let count: usize = fields[0].parse()?;
        let start: usize = fields[1].parse()?;
        if count > 480_000 || start >= count || fields.len() != 3 {
            return Err("invalid replay record".into());
        }
        let mut bytes = vec![0; count * 4];
        input.read_exact(&mut bytes)?;
        let samples: Vec<_> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| f32::from_le_bytes(*chunk))
            .collect();
        if let Some(decoded) = waveform.decode(&samples, start..count) {
            let payload = data::decode(4, &decoded.frame, fields[2]);
            writeln!(
                output,
                "{}\t{}\t{}\t{}",
                decoded.samples.start,
                decoded.samples.end,
                hex(&decoded.frame),
                payload.map_or_else(|| "-".into(), |frame| hex(&frame.payload))
            )?;
        } else {
            writeln!(output, "MISS")?;
        }
        output.flush()?;
        line.clear();
    }
    Ok(())
}
