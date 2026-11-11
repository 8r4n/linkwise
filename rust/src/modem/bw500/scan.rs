//! Rolling pilot acquisition for the three BW500 index waveforms.
use std::{collections::VecDeque, f32::consts::TAU, sync::Arc};

use rustfft::{Fft, FftPlanner, num_complex::Complex32};

use super::{Column, Level, Mode, SAMPLE_RATE_HZ, Tone, decode_aligned, reference_score};

const HOP: usize = 128;
const OFFSETS_HZ: [f32; 3] = [0.0, -12.0, 12.0];
const MIN_SCORE: f32 = 0.65;
const MAX_BINS: usize = 21;
// Longer than the complete level-1 frame, measured in scan hops.
const HISTORY: usize = 2000;
type Power = [[f32; MAX_BINS]; OFFSETS_HZ.len()];

struct Spectrum {
    fft: Arc<dyn Fft<f32>>,
    work: Vec<Complex32>,
    scratch: Vec<Complex32>,
    rotation: [Vec<Complex32>; OFFSETS_HZ.len()],
    first_bin: usize,
    bins: usize,
    powers: VecDeque<Power>,
}

impl Spectrum {
    fn new(planner: &mut FftPlanner<f32>, symbol: usize, first_bin: usize, bins: usize) -> Self {
        let fft = planner.plan_fft_forward(symbol);
        Self {
            scratch: vec![Complex32::default(); fft.get_inplace_scratch_len()],
            fft,
            work: vec![Complex32::default(); symbol],
            rotation: OFFSETS_HZ.map(|offset| {
                (0..symbol)
                    .map(|i| {
                        Complex32::from_polar(1.0, -TAU * offset * i as f32 / SAMPLE_RATE_HZ as f32)
                    })
                    .collect()
            }),
            first_bin,
            bins,
            powers: VecDeque::with_capacity(HISTORY),
        }
    }

    fn push(&mut self, samples: &[f32]) {
        let mut power = [[0.0; MAX_BINS]; OFFSETS_HZ.len()];
        for (row, rotation) in power.iter_mut().zip(&self.rotation) {
            for ((cell, phase), sample) in self.work.iter_mut().zip(rotation).zip(samples) {
                *cell = phase * *sample;
            }
            self.fft
                .process_with_scratch(&mut self.work, &mut self.scratch);
            let end = self.first_bin + self.bins;
            // Normalize against energy near the occupied band, including
            // guard bins so edge offsets cannot inflate a pilot score.
            let total: f32 = self.work[self.first_bin - 2..end + 2]
                .iter()
                .map(|bin| bin.norm_sqr())
                .sum();
            if total.is_finite() && total > f32::EPSILON {
                for (dst, bin) in row.iter_mut().zip(&self.work[self.first_bin..end]) {
                    *dst = bin.norm_sqr() / total;
                }
            }
        }
        if self.powers.len() == HISTORY {
            self.powers.pop_front();
        }
        self.powers.push_back(power);
    }

    fn scores(&self, profile: &Profile) -> [f32; OFFSETS_HZ.len()] {
        let Some(first) = self.powers.len().checked_sub(profile.history_needed) else {
            return [0.0; OFFSETS_HZ.len()];
        };
        let mut score = [0.0; OFFSETS_HZ.len()];
        for &(index, bin) in &profile.pilots {
            let column = &self.powers[first + index];
            for (sum, powers) in score.iter_mut().zip(column) {
                *sum += powers[bin];
            }
        }
        score.map(|value| value / profile.pilots.len() as f32)
    }
}

struct Profile {
    mode: Mode,
    pilots: Vec<(usize, usize)>,
    history_needed: usize,
    tones: [Vec<Tone>; OFFSETS_HZ.len()],
}

impl Profile {
    fn new(level: Level) -> Self {
        let mode = Mode::new(level);
        let hops_per_symbol = mode.symbol / HOP;
        let pilots = mode
            .columns
            .iter()
            .enumerate()
            .filter_map(|(i, column)| match column {
                Column::Reference(bin) => Some((i * hops_per_symbol, *bin)),
                Column::Data(_) => None,
            })
            .collect();
        let history_needed = (mode.columns.len() - 1) * hops_per_symbol + 1;
        let tones = OFFSETS_HZ.map(|offset| {
            let bins = f64::from(offset) * mode.symbol as f64 / f64::from(SAMPLE_RATE_HZ);
            (mode.first_bin..mode.first_bin + mode.span)
                .map(|bin| Tone::at_bin(bin as f64 + bins, mode.symbol))
                .collect()
        });
        Self {
            mode,
            pilots,
            history_needed,
            tones,
        }
    }
}

pub(crate) struct Scan {
    profiles: [Profile; 3],
    long: Spectrum,
    short: Spectrum,
    next_end: u64,
    last_end: u64,
}

impl Scan {
    pub(crate) fn new() -> Self {
        let profiles = [Level::One, Level::Two, Level::Three].map(Profile::new);
        let mut planner = FftPlanner::new();
        let [long, short, _] = &profiles;
        let spectrum = |planner: &mut FftPlanner<f32>, profile: &Profile| {
            let mode = &profile.mode;
            Spectrum::new(planner, mode.symbol, mode.first_bin, mode.span)
        };
        Self {
            long: spectrum(&mut planner, long),
            short: spectrum(&mut planner, short),
            profiles,
            next_end: HOP as u64,
            last_end: 0,
        }
    }

    pub(crate) fn push(
        &mut self,
        audio: &[f32],
        base: u64,
        mut emit: impl FnMut(crate::modem::receiver::ReceivedFrame),
    ) {
        while self.next_end <= base + audio.len() as u64 {
            let end = (self.next_end - base) as usize;
            self.next_end = self.next_end.saturating_add(HOP as u64);
            for spectrum in [&mut self.long, &mut self.short] {
                let length = spectrum.work.len();
                if end >= length {
                    spectrum.push(&audio[end - length..end]);
                }
            }
            for (i, profile) in self.profiles.iter().enumerate() {
                let length = profile.mode.samples_per_frame();
                let Some(nominal) = end.checked_sub(length) else {
                    continue;
                };
                // A decoded frame owns its interval. Allow small acquisition
                // jitter at the boundary of consecutive frames.
                if base + nominal as u64 + (HOP as u64) < self.last_end {
                    continue;
                }
                let spectrum = if i == 0 { &self.long } else { &self.short };
                let scores = spectrum.scores(profile);
                let mut offsets = [0, 1, 2];
                offsets.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]));
                for offset in offsets {
                    if scores[offset] < MIN_SCORE {
                        break;
                    }
                    let tones = &profile.tones[offset];
                    let first = nominal.saturating_sub(HOP);
                    // Refine only within samples available at this scan epoch.
                    let last = nominal;
                    let start = (first..=last)
                        .step_by(16)
                        .max_by(|&a, &b| {
                            let score = |start| {
                                reference_score(&profile.mode, tones, &audio[start..start + length])
                            };
                            score(a).total_cmp(&score(b))
                        })
                        .unwrap_or(nominal);
                    if base + start as u64 + (HOP as u64) < self.last_end {
                        continue;
                    }
                    let Some(frame) =
                        decode_aligned(&profile.mode, tones, &audio[start..start + length])
                    else {
                        continue;
                    };
                    let start = base + start as u64;
                    self.last_end = start + length as u64;
                    emit(crate::modem::receiver::ReceivedFrame {
                        level: profile.mode.level.number(),
                        samples: start..self.last_end,
                        frame,
                    });
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modem::{
        bw500::{Frame, synthesize},
        receiver::Receiver,
    };

    #[test]
    fn recovers_consecutive_frames_through_continuous_noise_without_duplicates() {
        let expected = [
            Frame {
                payload: vec![0x23; 22],
                control: 0x31,
            },
            Frame {
                payload: vec![0xa6; 22],
                control: 0x02,
            },
        ];
        let burst = synthesize(Level::Two, &expected).unwrap();
        let mut audio = vec![0.0; 911];
        audio.extend(burst.iter().map(|x| x * 0.14));
        audio.extend([0.0; 512]);
        // Unrelated above-gate audio joins the entire recording into one
        // candidate. The pilot matcher should still locate each frame.
        for (i, sample) in audio.iter_mut().enumerate() {
            *sample += 0.025 * (TAU * 930.0 * i as f32 / SAMPLE_RATE_HZ as f32).sin();
        }

        let replay = |size| {
            let mut receiver = Receiver::new().with_pilot_acquisition();
            let mut result = Vec::new();
            let mut add = |event: crate::modem::receiver::Reception| {
                result.extend(event.frames.into_iter().map(|f| (f.samples, f.frame)));
            };
            for chunk in audio.chunks(size) {
                receiver.push(chunk, &mut add).unwrap();
            }
            receiver.finish(add);
            result
        };
        let decoded = replay(1024);
        assert_eq!(
            decoded.iter().map(|(_, f)| f).collect::<Vec<_>>(),
            expected.iter().collect::<Vec<_>>()
        );
        assert_eq!(replay(137), decoded);
    }
}
