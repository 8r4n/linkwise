//! Offline DATA frame synthesis and reception for BW500 levels 1–3.

use std::ops::Range;

use super::{
    coding::{self, Random24},
    waveform::{Tone, energy},
};

pub(crate) mod scan;

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
        (self.first_bin..self.first_bin + self.span)
            .map(|bin| Tone::new(bin, self.symbol))
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
    let mut score = 0.0;
    for (column, chunk) in mode.columns.iter().zip(samples.chunks_exact(mode.symbol)) {
        if let Column::Reference(bin) = column {
            let norm = energy(chunk) * mode.symbol as f64 / 2.0;
            score += tones[*bin].power(chunk) / norm.max(f64::EPSILON);
        }
    }
    score / 24.0
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

fn decode_aligned(mode: &Mode, tones: &[Tone], samples: &[f32]) -> Option<Frame> {
    let mut coded = vec![0.0; mode.channel.len()];
    let mut channel = mode.channel.as_chunks::<3>().0.iter();
    for (column, chunk) in mode.columns.iter().zip(samples.chunks_exact(mode.symbol)) {
        if let Column::Data(bins) = column {
            let norm = (energy(chunk) * mode.symbol as f64 / 2.0).max(f64::EPSILON);
            let powers = bins.map(|bin| tones[bin].power(chunk) / norm);
            for (bit, &position) in channel.next().unwrap().iter().enumerate() {
                let mut best = [0.0_f64; 2];
                for (value, power) in powers.iter().enumerate() {
                    let choice = (value >> (2 - bit)) & 1;
                    best[choice] = best[choice].max(*power);
                }
                coded[position] = best[1] - best[0];
            }
        }
    }
    coding::decode(&coded, &mode.interleaver, mode.rate_third, |bits| {
        mode.unpack(bits)
    })
}

/// Finds CRC-valid BW500 DATA frames in a candidate burst in 48 kHz audio.
pub fn decode_burst(audio: &[f32], burst: Range<usize>) -> Vec<DecodedFrame> {
    let Some(samples) = audio.get(burst.clone()) else {
        return Vec::new();
    };
    for level in [Level::One, Level::Two, Level::Three] {
        let mode = Mode::new(level);
        // Allow one detector window of edge error at each end.
        if samples.len() + 960 < mode.samples_per_frame() {
            continue;
        }
        let tones = mode.tones();
        let Some(start) = acquire(&mode, &tones, audio, &burst) else {
            continue;
        };
        let end = burst.end.saturating_add(480).min(audio.len());
        let mut decoded = Vec::new();
        for offset in (start..end).step_by(mode.samples_per_frame()) {
            let frame_end = offset + mode.samples_per_frame();
            if frame_end > end {
                break;
            }
            if let Some(frame) = decode_frame(&mode, &tones, &audio[offset..frame_end]) {
                decoded.push(DecodedFrame {
                    level: mode.level,
                    samples: offset..frame_end,
                    frame,
                });
            }
        }
        if !decoded.is_empty() {
            return decoded;
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
