//! BW2300 and BW2750 setup and index-modulated DATA waveforms.
//!
//! The coding, pilot and allocation rules follow §§3.5 and 01 of Saul St
//! John's published VARA HF protocol measurements.

use std::ops::Range;

use super::{
    coding::{self, Random24},
    control::{self, Bandwidth},
    waveform::{Tone, energy},
};

pub const FRAME_BYTES: usize = 92;
pub const LOWER_FRAME_BYTES: usize = 50;
pub type Waveform = IndexWaveform<FRAME_BYTES, 1484>;
pub const ROBUST_FRAME_BYTES: usize = 25;
pub type RobustWaveform = IndexWaveform<ROBUST_FRAME_BYTES, 612>;
pub type LowerWaveform = IndexWaveform<LOWER_FRAME_BYTES, 816>;
mod measured {
    include!("wide_allocations.rs");
}
const GRAY: [usize; 16] = [0, 1, 3, 2, 7, 6, 4, 5, 15, 14, 12, 13, 8, 9, 11, 10];

pub struct IndexWaveform<const BYTES: usize, const CELLS: usize> {
    bandwidth: Bandwidth,
    span: usize,
    channel: Vec<usize>,
    turbo: Vec<usize>,
    symbol: usize,
    training: usize,
    stride: usize,
    bits: usize,
    allocation: Vec<usize>,
    pilots: Vec<Option<usize>>,
    tones: Vec<Tone>,
}

impl Waveform {
    pub fn new(bandwidth: Bandwidth) -> Option<Self> {
        let (first_bin, span) = match bandwidth {
            Bandwidth::Hz2300 => (9, 16),
            Bandwidth::Hz2750 => (6, 20),
            Bandwidth::Hz500 => return None,
        };
        let mut random = Random24(9_947_396);
        let channel = random.permutation(1484);
        let turbo = random.permutation(FRAME_BYTES * 8);
        let allocation: Vec<_> = (0..395).map(|_| random.draw(span)).collect();
        let mut pilots = vec![None; 395];
        for index in 0..24 {
            let column = (279 * index + 263) / 17;
            pilots[column] = Some((allocation[column] + 2 * random.draw(8)) % span);
        }
        let tones = (first_bin..first_bin + span)
            .map(|bin| Tone::new(bin, 512))
            .collect();
        Some(Self {
            bandwidth,
            span,
            channel,
            turbo,
            symbol: 512,
            training: 12,
            stride: 1,
            bits: 4,
            allocation,
            pilots,
            tones,
        })
    }

    /// Set the lead to the caller's setup position. The session owner can
    /// advance this stream for subsequent DATA bursts.
    pub fn setup_lead(&self, caller: &str) -> Option<Lead> {
        control::lead_stream(self.bandwidth, caller, false).map(Lead)
    }

    /// Initialize the caller's DATA lead after its level-4 setup preamble.
    pub fn caller_data_lead(&self, caller: &str) -> Option<Lead> {
        let mut lead = self.setup_lead(caller)?;
        self.training(&mut lead);
        Some(lead)
    }

    /// Initialize the responder's lead stream, which does not carry setup.
    pub fn responder_lead(&self, caller: &str) -> Option<Lead> {
        control::lead_stream(self.bandwidth, caller, true).map(Lead)
    }
}

impl LowerWaveform {
    pub fn new_level3(bandwidth: Bandwidth) -> Option<Self> {
        let (first_bin, span, bins) = match bandwidth {
            Bandwidth::Hz2300 => (17, 32, &measured::BW2300),
            Bandwidth::Hz2750 => (12, 40, &measured::BW2750),
            Bandwidth::Hz500 => return None,
        };
        Some(Self::lower(
            bandwidth,
            (first_bin, span),
            bins,
            14_955_604,
            402,
            6,
            4,
        ))
    }
}

impl RobustWaveform {
    pub fn new_level2(bandwidth: Bandwidth) -> Option<Self> {
        let (first_bin, span, bins) = match bandwidth {
            Bandwidth::Hz2300 => (17, 32, &measured::BW2300_L2),
            Bandwidth::Hz2750 => (12, 40, &measured::BW2750_L2),
            Bandwidth::Hz500 => return None,
        };
        Some(Self::lower(
            bandwidth,
            (first_bin, span),
            bins,
            7_910_701,
            200,
            4,
            3,
        ))
    }
}

impl<const BYTES: usize, const CELLS: usize> IndexWaveform<BYTES, CELLS> {
    fn lower(
        bandwidth: Bandwidth,
        geometry: (usize, usize),
        bins: &[usize],
        seed: u32,
        info: usize,
        training: usize,
        bits: usize,
    ) -> Self {
        let (first_bin, span) = geometry;
        let mut random = Random24(seed);
        let channel = random.permutation(CELLS);
        let turbo = random.permutation(info);
        let allocation = bins.to_vec();
        let mut pilots = vec![None; allocation.len()];
        for index in 0..24 {
            let column = (104 * index + 93) / 11;
            pilots[column] = Some(allocation[column]);
        }
        Self {
            bandwidth,
            span,
            channel,
            turbo,
            symbol: 1024,
            training,
            stride: 2,
            bits,
            allocation,
            pilots,
            tones: (first_bin..first_bin + span)
                .map(|bin| Tone::new(bin, 1024))
                .collect(),
        }
    }
    fn training(&self, lead: &mut Lead) -> Vec<usize> {
        (1..=self.training)
            .map(|index| {
                (self.allocation[index] + self.stride * lead.0.draw(1 << self.bits)) % self.span
            })
            .collect()
    }

    /// Render one DATA frame, consuming one training preamble from the lead.
    pub fn synthesize(&self, frame: &[u8; BYTES], lead: &mut Lead) -> Option<Vec<f32>> {
        let coded: [u8; CELLS] = coding::encode_frame(frame, &self.channel, &self.turbo)?;
        let mut groups = coded.chunks_exact(self.bits);
        let mut output = vec![0.0; self.symbol * 2];
        let training = self.training(lead);
        let columns =
            self.allocation
                .iter()
                .zip(self.pilots.iter().copied())
                .map(|(&allocation, pilot)| {
                    if let Some(bin) = pilot {
                        bin
                    } else {
                        let label = groups
                            .next()
                            .unwrap()
                            .iter()
                            .fold(0, |value, &bit| (value << 1) | usize::from(bit));
                        (allocation + self.stride * GRAY[label]) % self.span
                    }
                });
        for bin in training.into_iter().chain(columns) {
            output.extend(
                self.tones[bin]
                    .quadrature
                    .iter()
                    .map(|&(_, sin)| (0.3 * sin) as f32),
            );
        }
        output.resize(output.len() + self.symbol * 2, 0.0);
        Some(output)
    }

    /// Recover a CRC-checked frame from one energy-gated burst. The fixed
    /// pilot columns allow acquisition before the caller's identity is known.
    pub fn decode(&self, audio: &[f32], candidate: Range<usize>) -> Option<[u8; BYTES]> {
        if candidate.start >= candidate.end || candidate.end > audio.len() {
            return None;
        }
        let last = audio
            .len()
            .checked_sub((self.training + self.allocation.len()) * self.symbol)?;
        let from = candidate.start.saturating_sub(self.symbol * 2);
        let through = candidate.start.saturating_add(self.symbol * 2).min(last);
        let pilot_score = |start| {
            self.pilots
                .iter()
                .enumerate()
                .filter_map(|(column, &pilot)| Some((column, pilot?)))
                .map(|(column, bin)| {
                    let offset = start + (self.training + column) * self.symbol;
                    let samples = &audio[offset..offset + self.symbol];
                    let norm = energy(samples) * self.symbol as f64 / 2.0;
                    self.tones[bin].power(samples) / norm.max(f64::EPSILON)
                })
                .sum::<f64>()
                / 24.0
        };
        let (start, score) = (from..=through)
            .step_by(16)
            .map(|start| (start, pilot_score(start)))
            .filter(|(_, score)| score.is_finite())
            .max_by(|a, b| a.1.total_cmp(&b.1))?;
        if score < 0.55 {
            return None;
        }
        let mut cells = [0.0; CELLS];
        let mut groups = cells.chunks_exact_mut(self.bits);
        for (column, &allocation) in self.allocation.iter().enumerate() {
            if self.pilots[column].is_some() {
                continue;
            }
            let offset = start + (self.training + column) * self.symbol;
            let samples = &audio[offset..offset + self.symbol];
            let all_powers = GRAY.map(|gray| {
                self.tones[(allocation + self.stride * gray) % self.span].power(samples)
            });
            let powers = &all_powers[..1 << self.bits];
            let maximum = powers
                .iter()
                .copied()
                .fold(0.0_f64, f64::max)
                .max(f64::EPSILON);
            for (bit, cell) in groups.next()?.iter_mut().enumerate() {
                let mut best = [0.0_f64; 2];
                for (label, &power) in powers.iter().enumerate() {
                    let value = (label >> (self.bits - 1 - bit)) & 1;
                    best[value] = best[value].max(power);
                }
                *cell = 8.0 * (best[1] - best[0]) / maximum;
            }
        }
        coding::decode_frame(&cells, &self.channel, &self.turbo)
    }
}

/// Position in one station's session training sequence.
pub struct Lead(Random24);

impl Lead {
    /// The first DATA frame of a delivery advances once before its preamble.
    pub fn begin_delivery(&mut self) {
        self.0.advance();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modem::{
        data,
        setup::{LinkSetup, Profile},
    };

    #[test]
    fn lower_wide_records_round_trip_in_both_bandwidths() {
        for bandwidth in [Bandwidth::Hz2300, Bandwidth::Hz2750] {
            let base = Waveform::new(bandwidth).unwrap();
            let mut lead = base.caller_data_lead("KN6MQS").unwrap();
            let level2 = RobustWaveform::new_level2(bandwidth).unwrap();
            let frame = data::wide_lower_delivery_frame(0, &[0x5a; 22], "KN6MQS").unwrap();
            let audio = level2.synthesize(&frame, &mut lead).unwrap();
            assert_eq!(level2.decode(&audio, 2048..audio.len()), Some(frame));

            let level3 = LowerWaveform::new_level3(bandwidth).unwrap();
            let frame = data::wide_lower_delivery_frame(0, &[0xa5; 25], "KN6MQS").unwrap();
            let audio = level3.synthesize(&frame, &mut lead).unwrap();
            assert_eq!(level3.decode(&audio, 2048..audio.len()), Some(frame));
        }
    }

    #[test]
    fn wide_setup_round_trips_both_combs() {
        let setup = LinkSetup {
            caller: "AAAA1".into(),
            high_speed: false,
            requested_level: 4,
        };
        let frame = setup.encode(Profile::Wide).unwrap().try_into().unwrap();
        for bandwidth in [Bandwidth::Hz2300, Bandwidth::Hz2750] {
            let waveform = Waveform::new(bandwidth).unwrap();
            let mut lead = waveform.setup_lead(&setup.caller).unwrap();
            let training = waveform.training(&mut lead);
            if matches!(bandwidth, Bandwidth::Hz2300) {
                assert_eq!(
                    training.into_iter().map(|bin| bin + 9).collect::<Vec<_>>(),
                    [14, 24, 10, 21, 23, 21, 22, 24, 15, 24, 23, 15]
                );
            }
            let mut lead = waveform.setup_lead(&setup.caller).unwrap();
            let mut audio = vec![0.0; 321];
            audio.extend(waveform.synthesize(&frame, &mut lead).unwrap());
            let decoded = waveform.decode(&audio, 960..audio.len()).unwrap();
            assert_eq!(
                LinkSetup::decode(Profile::Wide, &decoded),
                Some(setup.clone())
            );
            let payload = b"Wide-mode DATA\x00\xff";
            let frame = crate::modem::data::wide_short(payload, &setup.caller).unwrap();
            lead.begin_delivery();
            let audio = waveform.synthesize(&frame, &mut lead).unwrap();
            let decoded = waveform.decode(&audio, 960..audio.len()).unwrap();
            let data = crate::modem::data::decode_wide(&decoded, &setup.caller).unwrap();
            assert_eq!(data.payload, payload);
        }
    }
}
