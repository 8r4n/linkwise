//! Offline DATA frame synthesis and reception for BW500 levels 1–3.

use std::ops::Range;

use super::{
    coding::{self, Random24},
    waveform::{Tone, energy},
};

pub(crate) mod scan;

#[cfg(test)]
mod sensitivity;

pub const SAMPLE_RATE_HZ: u32 = 48_000;
const LABELS: [usize; 8] = [0, 1, 3, 2, 7, 6, 4, 5];

// Recovered from stock level-1 arm 1 by removing the coded symbol offsets from
// the measured peak bins. Arm 2 and the two-frame capture independently check
// these values in the replay test.
const LEVEL1_BASE_BINS: [usize; 124] = [
    65, 62, 59, 58, 65, 60, 54, 68, 63, 72, 69, 68, 67, 72, 63, 74, 73, 58, 65, 64, 73, 54, 69, 60,
    73, 60, 63, 70, 55, 54, 69, 62, 57, 62, 71, 62, 71, 58, 57, 54, 67, 62, 65, 70, 65, 62, 67, 72,
    71, 74, 54, 62, 55, 70, 61, 72, 65, 58, 61, 58, 59, 56, 63, 68, 59, 56, 57, 58, 65, 60, 73, 56,
    71, 58, 71, 72, 71, 70, 67, 54, 63, 66, 69, 70, 57, 58, 55, 60, 73, 54, 67, 72, 54, 54, 54, 68,
    63, 56, 59, 58, 65, 64, 61, 56, 65, 68, 59, 64, 67, 72, 65, 64, 63, 68, 65, 58, 55, 60, 61, 68,
    67, 60, 54, 62,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Level {
    One,
    Two,
    Three,
}

impl Level {
    pub fn number(self) -> u8 {
        match self {
            Self::One => 1,
            Self::Two => 2,
            Self::Three => 3,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct Frame {
    /// Full wire payload field. Any session padding is interpreted by ARQ.
    pub payload: Vec<u8>,
    pub control: u8,
}

#[derive(Debug, Eq, PartialEq)]
pub struct DecodedFrame {
    pub level: Level,
    pub samples: Range<usize>,
    pub frame: Frame,
}

struct Mode {
    level: Level,
    symbol: usize,
    first_bin: usize,
    span: usize,
    lead: usize,
    info_bits: usize,
    rate_third: bool,
    channel: Vec<usize>,
    interleaver: Vec<usize>,
    columns: Vec<Column>,
}

enum Column {
    Reference(usize),
    Data([usize; 8]),
}

impl Mode {
    fn new(level: Level) -> Self {
        let (seed, count, info_bits, symbol, first_bin, span, stride, lead, pilots) = match level {
            Level::One => (12_600_710, 124, 96, 2048, 54, 21, 2, 2, (41, 33, 8)),
            Level::Two => (7_910_701, 228, 200, 1024, 27, 11, 1, 4, (104, 93, 11)),
            Level::Three => (14_955_604, 226, 297, 1024, 27, 11, 1, 4, (75, 67, 8)),
        };
        let mut random = Random24(seed);
        let channel = random.permutation((count - 24) * 3);
        let interleaver = random.permutation(info_bits);
        let mut bases: Vec<_> = (0..count).map(|_| first_bin + random.draw(span)).collect();
        if level == Level::One {
            bases.copy_from_slice(&LEVEL1_BASE_BINS);
        }

        let bin = |base, label| (base - first_bin + stride * label) % span;
        let mut columns: Vec<_> = bases
            .iter()
            .map(|&base| Column::Data(LABELS.map(|label| bin(base, label))))
            .collect();
        let (a, b, denominator) = pilots;
        for k in 0..24 {
            let column = (a * k + b) / denominator;
            columns[column] = Column::Reference(bin(bases[column], random.draw(8)));
        }

        Self {
            level,
            symbol,
            first_bin,
            span,
            lead,
            info_bits,
            rate_third: level != Level::Three,
            channel,
            interleaver,
            columns,
        }
    }

    fn samples_per_frame(&self) -> usize {
        self.columns.len() * self.symbol
    }

    fn unpack(&self, bits: &[u8]) -> Option<Frame> {
        let mut bits = bits.to_vec();
        coding::whiten(&mut bits);
        let bytes: Vec<_> = bits
            .as_chunks::<8>()
            .0
            .iter()
            .map(|octet| octet.iter().fold(0, |byte, &bit| byte * 2 + bit))
            .collect();
        let body_len = bytes.len() - 2;
        let check = u16::from_be_bytes([bytes[body_len], bytes[body_len + 1]]);
        (coding::crc(&bytes[..body_len]) == check).then(|| Frame {
            payload: bytes[..body_len - 1].to_vec(),
            control: bytes[body_len - 1],
        })
    }

    fn tones(&self) -> Vec<Tone> {
        self.tones_at(0.0)
    }

    fn tones_at(&self, hz: f64) -> Vec<Tone> {
        let bins = hz * self.symbol as f64 / f64::from(SAMPLE_RATE_HZ);
        (self.first_bin..self.first_bin + self.span)
            .map(|bin| Tone::at_bin(bin as f64 + bins, self.symbol))
            .collect()
    }
}

/// Renders full DATA fields as 48 kHz mono samples for offline testing.
/// The initial training columns use fixed in-band tones.
pub fn synthesize(level: Level, frames: &[Frame]) -> Option<Vec<f32>> {
    let mode = Mode::new(level);
    if frames.is_empty()
        || frames
            .iter()
            .any(|frame| frame.payload.len() != mode.info_bits / 8 - 3)
    {
        return None;
    }
    let tones = mode.tones();
    let mut output = Vec::new();
    let mut append_tone = |bin: usize| {
        output.extend(
            tones[bin]
                .quadrature
                .iter()
                .map(|(_, sine)| (sine * 0.5) as f32),
        );
    };
    for i in 0..mode.lead {
        append_tone((mode.span / 2 + i) % mode.span);
    }
    for frame in frames {
        let mut bytes = frame.payload.clone();
        bytes.push(frame.control);
        bytes.extend(coding::crc(&bytes).to_be_bytes());
        let mut bits: Vec<_> = bytes
            .iter()
            .flat_map(|byte| (0..8).rev().map(move |shift| (byte >> shift) & 1))
            .collect();
        bits.resize(mode.info_bits, 0);
        coding::whiten(&mut bits);
        let coded = coding::encode(&bits, &mode.interleaver, mode.rate_third);
        let mut channel = mode.channel.as_chunks::<3>().0.iter();
        for column in &mode.columns {
            let bin = match column {
                Column::Reference(bin) => *bin,
                Column::Data(bins) => {
                    let value = channel
                        .next()
                        .unwrap()
                        .iter()
                        .fold(0, |value, &index| value * 2 + usize::from(coded[index]));
                    bins[value]
                }
            };
            append_tone(bin);
        }
    }
    Some(output)
}

fn reference_score(mode: &Mode, tones: &[Tone], samples: &[f32]) -> f64 {
    pilot_score(mode, tones, samples, 1)
}

fn pilot_score(mode: &Mode, tones: &[Tone], samples: &[f32], stride: usize) -> f64 {
    mode.columns
        .iter()
        .zip(samples.chunks_exact(mode.symbol))
        .filter_map(|(column, chunk)| match column {
            Column::Reference(bin) => Some((*bin, chunk)),
            Column::Data(_) => None,
        })
        .step_by(stride)
        .map(|(bin, chunk)| {
            let norm = energy(chunk) * mode.symbol as f64 / 2.0;
            tones[bin].power(chunk) / norm.max(f64::EPSILON)
        })
        .sum::<f64>()
        / 24_usize.div_ceil(stride) as f64
}

fn shifted_tones(mode: &Mode, audio: &[f32], start: usize) -> Option<Vec<Tone>> {
    let nominal = start + mode.lead * mode.symbol;
    let range = nominal.saturating_sub(mode.symbol)..=nominal + mode.symbol;
    [-8.0, 8.0, -15.0, 15.0, -22.0, 22.0]
        .into_iter()
        .map(|hz| {
            let tones = mode.tones_at(hz);
            let score = range
                .clone()
                .step_by(256)
                .filter_map(|offset| audio.get(offset..offset + mode.samples_per_frame()))
                .map(|samples| pilot_score(mode, &tones, samples, 3))
                .max_by(f64::total_cmp)
                .unwrap_or(0.0);
            (tones, score)
        })
        .filter(|(_, score)| *score >= 0.50)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(tones, _)| tones)
}

fn acquire(mode: &Mode, tones: &[Tone], audio: &[f32], burst: &Range<usize>) -> Option<usize> {
    let nominal = burst.start + mode.lead * mode.symbol;
    let start = nominal.saturating_sub(mode.symbol);
    let end = nominal + mode.symbol;
    let score = |offset: usize| {
        audio
            .get(offset..offset + mode.samples_per_frame())
            .map_or(0.0, |samples| reference_score(mode, tones, samples))
    };
    // References supply the symbol epoch without looking at expected payloads.
    let coarse = (start..=end)
        .step_by(128)
        .map(|offset| (offset, score(offset)))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    let fine = (coarse.0.saturating_sub(128)..=coarse.0 + 128)
        .step_by(16)
        .map(|offset| (offset, score(offset)))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    (fine.1 >= 0.65).then_some(fine.0)
}

fn decode_frame(mode: &Mode, tones: &[Tone], samples: &[f32]) -> Option<Frame> {
    if reference_score(mode, tones, samples) < 0.65 {
        return None;
    }
    decode_aligned(mode, tones, samples)
}


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
 if matches!(method.as_str(),"gated"|"iterated"|"stable"){
  if let Some(frame)=decode(&research_metrics(mode,tones,samples,&weights)){return Some(frame);}
  GATE_ATTEMPTS.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
 }
 let (floors,gate)=research_floors(mode,tones,samples);
 if matches!(method.as_str(),"gated"|"iterated"|"stable") && !gate {return None;}
 if matches!(method.as_str(),"gated"|"iterated"|"stable") {GATE_ACTIVE.fetch_add(1,std::sync::atomic::Ordering::Relaxed);}
 let common=research_median(floors.clone());
 let weights:Vec<_>=floors.iter().map(|f|if matches!(method.as_str(),"gated"|"iterated"|"stable"){(common/f).clamp(0.1,1.0)}else{common/f}).collect();
 let metrics=research_metrics(mode,tones,samples,&weights);
 if method=="iterated" || method=="stable" {
  coding::decode_confirmed(&metrics,&mode.interleaver,mode.rate_third,|bits|mode.unpack(bits),method=="stable")
 }else{decode(&metrics)}
}
pub fn research_branch(audio:&[f32],start:usize,hz:f64)->Vec<f64>{
 let mode=Mode::new(Level::Two);research_metrics(&mode,&mode.tones_at(hz),&audio[start..start+mode.samples_per_frame()],&vec![1.0;mode.span])
}
pub fn research_decode(metrics:&[f64])->Option<Frame>{
 let mode=Mode::new(Level::Two);coding::decode(metrics,&mode.interleaver,mode.rate_third,|bits|mode.unpack(bits))
}
/// Finds CRC-valid BW500 DATA frames in a candidate burst in 48 kHz audio.
pub fn decode_burst(audio: &[f32], burst: Range<usize>) -> Vec<DecodedFrame> {
    let Some(samples) = audio.get(burst.clone()) else {
        return Vec::new();
    };
    let modes = [Level::One, Level::Two, Level::Three]
        .map(Mode::new)
        .into_iter()
        // Allow one detector window of edge error at each end.
        .filter(|mode| samples.len() + 960 >= mode.samples_per_frame());
    let modes: Vec<_> = modes.collect();
    for shifted in [false, true] {
        for mode in &modes {
            let tones = if shifted {
                let Some(tones) = shifted_tones(mode, audio, burst.start) else {
                    continue;
                };
                tones
            } else {
                mode.tones()
            };
            let Some(start) = acquire(mode, &tones, audio, &burst) else {
                continue;
            };
            let end = burst.end.saturating_add(480).min(audio.len());
            let decoded: Vec<_> = (start..end)
                .step_by(mode.samples_per_frame())
                .map(|offset| offset..offset + mode.samples_per_frame())
                .take_while(|frame| frame.end <= end)
                .filter_map(|samples| {
                    let frame = decode_frame(mode, &tones, &audio[samples.clone()])?;
                    Some(DecodedFrame {
                        level: mode.level,
                        samples,
                        frame,
                    })
                })
                .collect();
            if !decoded.is_empty() {
                return decoded;
            }
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthesized_audio_round_trips_at_each_level() {
        for (level, size) in [(Level::One, 9), (Level::Two, 22), (Level::Three, 34)] {
            let frames = [
                Frame {
                    payload: (0..size).collect(),
                    control: 0x99,
                },
                Frame {
                    payload: vec![0xff; size as usize],
                    control: 0x95,
                },
            ];
            let mut audio = vec![0.0; 3217];
            audio.extend(synthesize(level, &frames).unwrap());
            let decoded = decode_burst(&audio, 3120..audio.len());
            assert_eq!(decoded.len(), 2, "{level:?}");
            for (decoded, frame) in decoded.iter().zip(frames) {
                assert_eq!(decoded.level, level);
                assert_eq!(decoded.frame, frame);
            }
        }
    }
}
