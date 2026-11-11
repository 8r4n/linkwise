//! BW2300 and BW2750 base-rate setup and DATA waveforms.
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
const CELLS: usize = 1484;
const SYMBOL: usize = 512;
const TRAINING: usize = 12;
const COLUMNS: usize = 395;
const BODY_SAMPLES: usize = (TRAINING + COLUMNS) * SYMBOL;
const GRAY: [usize; 16] = [0, 1, 3, 2, 7, 6, 4, 5, 15, 14, 12, 13, 8, 9, 11, 10];

pub struct Waveform {
    bandwidth: Bandwidth,
    span: usize,
    channel: Vec<usize>,
    turbo: Vec<usize>,
    allocation: [usize; COLUMNS],
    pilots: [Option<usize>; COLUMNS],
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
        let channel = random.permutation(CELLS);
        let turbo = random.permutation(FRAME_BYTES * 8);
        let allocation = std::array::from_fn(|_| random.draw(span));
        let mut pilots = [None; COLUMNS];
        for index in 0..24 {
            let column = (279 * index + 263) / 17;
            pilots[column] = Some((allocation[column] + 2 * random.draw(8)) % span);
        }
        let tones = (first_bin..first_bin + span)
            .map(|bin| Tone::new(bin, SYMBOL))
            .collect();
        Some(Self {
            bandwidth,
            span,
            channel,
            turbo,
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

    fn training(&self, lead: &mut Lead) -> [usize; TRAINING] {
        std::array::from_fn(|index| (self.allocation[index + 1] + lead.0.draw(16)) % self.span)
    }

    /// Render one level-4 frame, consuming one training preamble from the lead.
    pub fn synthesize(&self, frame: &[u8; FRAME_BYTES], lead: &mut Lead) -> Option<Vec<f32>> {
        let coded: [u8; CELLS] = coding::encode_frame(frame, &self.channel, &self.turbo)?;
        let mut groups = coded.as_chunks::<4>().0.iter();
        let mut output = vec![0.0; SYMBOL * 2];
        let training = self.training(lead);
        let columns = self
            .allocation
            .iter()
            .zip(self.pilots)
            .map(|(&allocation, pilot)| {
                if let Some(bin) = pilot {
                    bin
                } else {
                    let label = groups
                        .next()
                        .unwrap()
                        .iter()
                        .fold(0, |value, &bit| (value << 1) | usize::from(bit));
                    (allocation + GRAY[label]) % self.span
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
        output.resize(output.len() + SYMBOL * 2, 0.0);
        Some(output)
    }

    /// Recover a CRC-checked frame from one energy-gated burst. The fixed
    /// pilot columns allow acquisition before the caller's identity is known.
    pub fn decode(&self, audio: &[f32], candidate: Range<usize>) -> Option<[u8; FRAME_BYTES]> {
        if candidate.start >= candidate.end || candidate.end > audio.len() {
            return None;
        }
        let last = audio.len().checked_sub(BODY_SAMPLES)?;
        let from = candidate.start.saturating_sub(SYMBOL * 2);
        let through = candidate.start.saturating_add(SYMBOL * 2).min(last);
        let pilot_score = |start| {
            self.pilots
                .iter()
                .enumerate()
                .filter_map(|(column, &pilot)| Some((column, pilot?)))
                .map(|(column, bin)| {
                    let offset = start + (TRAINING + column) * SYMBOL;
                    let samples = &audio[offset..offset + SYMBOL];
                    let norm = energy(samples) * SYMBOL as f64 / 2.0;
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
        let mut groups = cells.as_chunks_mut::<4>().0.iter_mut();
        for (column, &allocation) in self.allocation.iter().enumerate() {
            if self.pilots[column].is_some() {
                continue;
            }
            let offset = start + (TRAINING + column) * SYMBOL;
            let samples = &audio[offset..offset + SYMBOL];
            let powers =
                GRAY.map(|gray| self.tones[(allocation + gray) % self.span].power(samples));
            let maximum = powers
                .iter()
                .copied()
                .fold(0.0_f64, f64::max)
                .max(f64::EPSILON);
            for (bit, cell) in groups.next()?.iter_mut().enumerate() {
                let mut best = [0.0_f64; 2];
                for (label, &power) in powers.iter().enumerate() {
                    let value = (label >> (3 - bit)) & 1;
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
    use crate::modem::setup::{LinkSetup, Profile};

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
                    training.map(|bin| bin + 9),
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
